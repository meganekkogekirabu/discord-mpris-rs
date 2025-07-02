use discordipc::{activity::{Activity, ActivityType, Assets}, Client, packet::Packet};
use dotenv::dotenv;
use mpris::{MetadataValue, PlayerFinder, PlaybackStatus};
use musicbrainz_rs::entity::release::{Release, ReleaseSearchQuery};
use musicbrainz_rs::prelude::*;
use once_cell::sync::Lazy;
use regex::{Regex, escape};
use std::env;
use std::error::Error;
use std::sync::{Mutex, MutexGuard, Arc};
use std::time::Duration;
use tokio::time::sleep;

async fn get_cover_art(current: Current) -> Result<String, Box<dyn Error>> {
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

impl Eq for Current {}

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

static CURRENT: Lazy<Mutex<Current>> = Lazy::new(|| {
    Mutex::new(Current::default())
});

fn set_current(new: Current) {
    let mut current = CURRENT.lock().unwrap();
    *current = new;
}

fn read_current() -> MutexGuard<'static, Current> {
    CURRENT.lock().unwrap()
}

fn reset_current() {
    let mut current = CURRENT.lock().unwrap();
    *current = Current::default();
}

static FILTER: Lazy<Regex> = Lazy::new(|| {
    Regex::new("^.*?\\{([^}]+)\\}.*?$").unwrap()
});

async fn process_metadata() -> Result<Current, String> {
    let ignore: Vec<String> = env::var("ignored_players")
        .map_err(|e| e.to_string())?
        .split(",")
        .map(|l| l.to_string())
        .collect();

    let show_paused = env::var("show_paused").map_err(|e| e.to_string())?;
    let show_stopped = env::var("show_stopped").map_err(|e| e.to_string())?;

    let mut players = PlayerFinder::new()
        .expect("could not connect to D-Bus")
        .find_all()
        .map_err(|e| e.to_string())?;

    players.retain(|p| !ignore.contains(&p.identity().to_string()));

    if players.len() == 0 {
        return Err("no players are active".to_string());
    }

    let player = &players[0]; // just get the first one, since with .find_active(), players can't be ignored

    let playback_status = player.get_playback_status().map_err(|e| e.to_string())?;
    
    let mut player_name = player.identity().to_string().to_lowercase();

    if show_stopped == "true" && playback_status == PlaybackStatus::Stopped {
        return Ok(Current::new(Some(ActivityInfo {
            details: "Stopped playback".into(),
            state: "".into(),
            subtitle: "".into(),
            image: player_name.into(),
        })));
    }

    if (playback_status == PlaybackStatus::Paused && show_paused == "false") || playback_status == PlaybackStatus::Stopped {
        return Err("no song is currently playing".to_string());
    }
    
    let metadata = player.get_metadata().expect("could not get metadata");

    let current = read_current();

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

    let rows: String = env::var("rows")
        .map_err(|e| e.to_string())?;

    let mut ret = Vec::with_capacity(4);
    
    for raw_row in rows.split(",").take(3) {
        let field = FILTER.replace_all(raw_row, "$1").to_string();
        let key = format!("xesam:{field}");

        if let Some(val) = metadata.get(&key) {
            ret.push(raw_row.replace(&format!("{{{field}}}"), &value_to_string(val)));
        } else {
            return Err(format!("could not get value for {field}"));
        }
    }

    while ret.len() < 3 {
        ret.push(String::new());
    }

    if playback_status == PlaybackStatus::Paused && show_paused == "true" {
        player_name.push_str("_paused");
    }

    let fetch_cover_art = env::var("fetch_cover_art").map_err(|e| e.to_string())?;

    if fetch_cover_art == "true" {
        if let Ok(url) = get_cover_art(new.clone()).await {
            new.url = url;
        } else {
            new.url = player_name;
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

    let interval = Duration::from_millis(
        env::var("update_interval")?.parse()
            .expect("update_interval must be a valid integer")
    );

    let application = env::var("application_id").map_err(|e| e.to_string())?;

    let client = Client::new_simple(application);
    client.connect_and_wait()?.filter()?;
    
    let mut listening = false;

    loop {
        match process_metadata().await {
            Ok(new) => {
                listening = true;
                let rows = &new.activity;

                if *read_current() != new {
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