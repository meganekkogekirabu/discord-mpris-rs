use discordipc::{activity::{Activity, ActivityType, Assets}, Client, packet::Packet};
use dotenv::dotenv;
use mpris::{MetadataValue, PlaybackStatus, PlayerFinder};
use musicbrainz_rs::entity::release::{Release, ReleaseSearchQuery};
use musicbrainz_rs::prelude::*;
use once_cell::sync::Lazy;
use regex::{Regex, escape};
use std::collections::HashMap;
use std::env;
use std::num::ParseIntError;
use std::error::Error;
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use std::time::Duration;
use tokio::time::sleep;

#[derive(Debug, thiserror::Error)]
enum AppError {
    #[error("Environment variable error: {0}")]
    EnvVar(#[from] env::VarError),

    #[error("DBus error: {0}")]
    DBus(#[from] mpris::DBusError),

    #[error("Finding error: {0}")]
    Finding(#[from] mpris::FindingError),

    #[error("MusicBrainz error: {0}")]
    MusicBrainz(#[from] musicbrainz_rs::Error),

    #[error("No active players")]
    NoActivePlayers,

    #[error("Failed to parse integer: {0}")]
    ParseInt(#[from] ParseIntError),

    #[error("Type mismatch for key {0}")]
    TypeMismatch(String),

    #[error("Error: {0}")]
    ParseError(#[from] Box<dyn Error>),

    #[error("No song is playing")]
    NoSongPlaying,

    #[error("Field not found: {0}")]
    FieldNotFound(String),
}

#[derive(Debug, Default)]
struct Config {
    cache: RwLock<HashMap<String, ConfigValue>>,
}

#[derive(Debug, Clone)]
enum ConfigValue {
    Vec(Vec<String>),
    String(String),
    Bool(bool),
    Duration(Duration),
}

static CONFIG: Lazy<Mutex<Config>> = Lazy::new(|| {
    Mutex::new(Config::new())
});

impl Config {
    pub fn new() -> Self {
        Config {
            cache: RwLock::new(HashMap::new()),
        }
    }

    pub fn get<T: TryFrom<ConfigValue>>(&self, key: &str) -> Result<T, AppError> {
        // check cache first
        {
            let cache = self.cache.read().unwrap();
            if let Some(val) = cache.get(key) {
                return T::try_from(val.clone())
                    .map_err(|_| AppError::TypeMismatch(key.to_string()));
            }
        }

        let env_value = env::var(key)?;
        let parsed_value = Self::parse_value(key, &env_value)?;

        // cache parsed value
        {
            let mut cache = self.cache.write().unwrap();
            cache.insert(key.to_string(), parsed_value.clone());
        }

        T::try_from(parsed_value).map_err(|_| AppError::TypeMismatch(key.to_string()))
    }

    fn parse_value(key: &str, raw: &str) -> Result<ConfigValue, Box<dyn Error>> {
        if let Ok(b) = raw.parse::<bool>() {
            Ok(ConfigValue::Bool(b))

        } else if key == "ignored_players" || key == "rows" {
            let vec = raw.split(',').map(|s| s.trim().to_string()).collect();
            Ok(ConfigValue::Vec(vec))

        } else if key == "update_interval" {
            let duration = Duration::from_millis(raw.parse::<u64>()?);
            Ok(ConfigValue::Duration(duration))
            
        } else {
            Ok(ConfigValue::String(raw.to_string()))
        }
    }
}

fn read_config() -> MutexGuard<'static, Config> {
    CONFIG.lock().unwrap()
}

impl TryFrom<ConfigValue> for bool {
    type Error = ();

    fn try_from(value: ConfigValue) -> Result<Self, Self::Error> {
        match value {
            ConfigValue::Bool(b) => Ok(b),
            _ => Err(()),
        }
    }
}

impl TryFrom<ConfigValue> for String {
    type Error = ();

    fn try_from(value: ConfigValue) -> Result<Self, Self::Error> {
        match value {
            ConfigValue::String(s) => Ok(s),
            _ => Err(()),
        }
    }
}

impl TryFrom<ConfigValue> for Vec<String> {
    type Error = ();

    fn try_from(value: ConfigValue) -> Result<Self, Self::Error> {
        match value {
            ConfigValue::Vec(s) => Ok(s),
            _ => Err(()),
        }
    }
}

impl TryFrom<ConfigValue> for Duration {
    type Error = ();

    fn try_from(value: ConfigValue) -> Result<Self, Self::Error> {
        match value {
            ConfigValue::Duration(s) => Ok(s),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Default)]
struct CoverArt {
    cache: RwLock<HashMap<String, String>>,
}

impl CoverArt {
    pub fn cache(&self, release: String, artist: String, url: String) {
        {
            let mut cache = self.cache.write().unwrap();
            cache.insert(format!("{release}_{artist}"), url);
        }
    }

    pub fn has(&self, key: &str) -> bool {
        {
            let cache = self.cache.read().unwrap();
            if let Some(_) = cache.get(key) {
                return true;
            } else {
                return false;
            }
        }
    }

    pub fn get(&self, key: String) -> String {
        {
            let cache = self.cache.read().unwrap();
            
            if let Some(v) = cache.get(&key) {
                return v.to_string();
            } else {
                return String::new();
            }
        }
    }
}

static COVER_ART_CACHE: Lazy<Mutex<CoverArt>> = Lazy::new(|| {
    Mutex::new(CoverArt {
        cache: RwLock::new(HashMap::new()),
    })
});

async fn get_cover_art(current: Current) -> Result<String, Box<dyn Error>> {
    let cover_art = COVER_ART_CACHE.lock().unwrap();
    let key = format!("{}_{}", current.release, current.artist);

    if cover_art.has(&key) {
        return Ok(cover_art.get(key));
    }

    let query = ReleaseSearchQuery::query_builder()
        .release(&escape(&current.release))
        .and()
        .artist(&escape(&current.artist))
        .build();

    let results = Release::search(query)
        .execute()
        .await?;

    if let Some(release) = results.entities.first() {
        let mbid = &release.id;

        if mbid == "1735e086-462e-42c3-b615-eebbd5e9f352" { // Nagios check release. This is what gets returned for "", "".
            return Err("could not find cover art".into());
        }

        let url = format!("https://coverartarchive.org/release/{mbid}/front");
        cover_art.cache(current.release, current.artist, url.clone());
        
        drop(cover_art);

        return Ok(url);
    } else {
        return Err(format!("could not find release {}", current.release).into());
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
struct ActivityInfo {
    details: Arc<str>,  // 1st row
    state: Arc<str>,    // 2nd row
    subtitle: Arc<str>, // 3rd row
    image: Arc<str>,    // Cover Art Archive URL or media player name
}

impl ActivityInfo {
    fn is_empty(&self) -> bool {
        self.details.is_empty()
            && self.state.is_empty()
            && self.subtitle.is_empty()
            && self.image.is_empty()
    }
}

impl Default for ActivityInfo {
    fn default() -> Self {
        ActivityInfo {
            details: "".into(),
            state: "".into(),
            subtitle: "".into(),
            image: "".into(),
        }
    }
}

fn value_to_string(val: &MetadataValue) -> String {
    match val {
        // add more as necessary
        MetadataValue::String(s) => s.clone(),
        MetadataValue::Array(arr) => {
            arr.iter().map(value_to_string).collect::<Vec<_>>().join(", ")
        }

        // fallback
        _ => "<unsupported>".to_string(),
    }
}

#[derive(Debug, Clone)]
struct Current {
    release: String,
    artist: String,
    url: String,
    activity: ActivityInfo,
}

impl PartialEq for Current {
    // Ignore activity in comparisons.
    fn eq(&self, other: &Self) -> bool {
        self.release == other.release
            && self.artist == other.artist
            && self.url == other.url
    }
}

impl Current {
    fn new(activity: Option<ActivityInfo>) -> Self {
        Current {
            release: String::new(),
            artist: String::new(),
            url: String::new(),
            activity: activity.unwrap_or_else(ActivityInfo::default),
        }
    }
}

impl Default for Current {
    fn default() -> Self {
        Current::new(None)
    }
}

static CURRENT: Lazy<Mutex<Current>> = Lazy::new(|| {
    Mutex::new(Current::default())
});

fn set_current(new: Current) {
    let mut current = CURRENT.lock().unwrap();
    *current = new;
}

fn reset_current() {
    let mut current = CURRENT.lock().unwrap();
    *current = Current::default();
}

static FILTER: Lazy<Regex> = Lazy::new(|| {
    Regex::new("^.*?\\{([^}]+)\\}.*?$").unwrap()
});

async fn process_metadata() -> Result<Current, AppError> {
    let config = CONFIG.lock().unwrap();
    let ignored_players: Vec<String> = config.get("ignored_players")?;

    let show_paused: bool = config.get("show_paused")?;
    let show_stopped: bool = config.get("show_stopped")?;

    let mut players = PlayerFinder::new()?
        .find_all()?;

    players.retain(|p| !ignored_players.contains(&p.identity().to_string()));

    if players.len() == 0 {
        return Err(AppError::NoActivePlayers);
    }

    let player = &players[0]; // just get the first one, since with .find_active(), players can't be ignored

    let playback_status = player.get_playback_status()?;
    
    let mut player_name = player.identity().to_string().to_lowercase();

    if show_stopped && playback_status == PlaybackStatus::Stopped {
        return Ok(Current::new(Some(ActivityInfo {
            details: "Stopped playback".into(),
            state: "".into(),
            subtitle: "".into(),
            image: player_name.into(),
        })));
    }

    if (playback_status == PlaybackStatus::Paused && !show_paused) || playback_status == PlaybackStatus::Stopped {
        return Err(AppError::NoSongPlaying);
    }
    
    let metadata = player.get_metadata().expect("could not get metadata");

    let current = CURRENT.lock().unwrap();

    let mut new = Current {
        release: metadata.album_name().unwrap().to_string(),
        artist: metadata.album_artists().unwrap().join(", "),
        url: current.url.clone(),
        activity: ActivityInfo::default(),
    };

    if new.release == current.release && !current.activity.is_empty() {
        return Ok(current.clone());
    }

    drop(current);

    let rows: Vec<String> = config.get("rows")?;

    let mut ret = Vec::with_capacity(4);
    
    for raw_row in rows.into_iter().take(3) {
        let field = FILTER.replace_all(&raw_row, "$1").to_string();
        let key = format!("xesam:{field}");

        if let Some(val) = metadata.get(&key) {
            ret.push(raw_row.replace(&format!("{{{field}}}"), &value_to_string(val)));
        } else {
            return Err(AppError::FieldNotFound(field));
        }
    }

    while ret.len() < 3 {
        ret.push(String::new());
    }

    if playback_status == PlaybackStatus::Paused && show_paused {
        player_name.push_str("_paused");
    }

    let fetch_cover_art: bool = config.get("fetch_cover_art")?;

    drop(config);

    if fetch_cover_art {
        match get_cover_art(new.clone()).await {
            Ok(url) => {
                new.url = url;
            },
            Err(_) => {
                new.url = player_name; // fall back to the icon for the media player
            }
        }
    } else {
        new.url = player_name;
    }
    
    ret.push(new.url.clone());

    assert_eq!(ret.len(), 4);

    new.activity = ActivityInfo {
        details: Arc::from(ret[0].as_str()),
        state: Arc::from(ret[1].as_str()),
        subtitle: Arc::from(ret[2].as_str()),
        image: Arc::from(ret[3].as_str()),
    };

    Ok(new)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenv().ok();

    let interval = read_config().get("update_interval")?;

    let application: String = read_config().get("application_id")?;

    let client = Client::new_simple(application);
    client.connect_and_wait()?.filter()?;
    
    let mut listening = false;

    loop {
        match process_metadata().await {
            Ok(new) => {
                listening = true;
                let rows = &new.activity;

                if *CURRENT.lock().unwrap() != new {
                    set_current(new.clone());

                    let activity = Activity::new()
                        .kind(ActivityType::Listening)
                        .details(&*rows.details)
                        .state(&*rows.state)
                        .assets(Assets::new()
                            .large_image(&*rows.image, Some(&*rows.subtitle)));

                    let activity_packet = Packet::new_activity(Some(&activity), None);

                    if let Err(why) = client.send_and_wait(activity_packet)?.filter() {
                        eprintln!("couldn't set activity: {why}");
                    }
                }
            },
            Err(_) => {
                if listening != false {
                    listening = false;
                    reset_current();
                    let _ = client.send_and_wait(Packet::new_activity(None, None))?.filter(); // send a blank packet to clear the rich presence
                }
            }
        }

        sleep(interval).await;
    }
}