use discordipc::{activity::{Activity, ActivityType, Assets}, Client, packet::Packet};
use dotenv::dotenv;
use mpris::{MetadataValue, PlayerFinder, PlaybackStatus};
use musicbrainz_rs::entity::release::{Release, ReleaseSearchQuery};
use musicbrainz_rs::prelude::*;
use once_cell::sync::Lazy;
use regex::Regex;
use std::{env, sync::{Mutex, MutexGuard}, thread};
use std::error::Error;
use std::time::Duration;

async fn get_cover_art(current: Current) -> Result<String, Box<dyn Error>> {
    let query = ReleaseSearchQuery::query_builder()
        .release(&current.release)
        .and()
        .artist(&current.artist)
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

struct ActivityInfo {
    details: String,  // 1st row
    state: String,    // 2nd row
    subtitle: String, // 3rd row
    image: String,    // Cover Art Archive URL or media player name
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

#[derive(Debug, PartialEq, Eq, Clone)]
struct Current {
    release: String,
    artist: String,
    url: String,
}

static CURRENT: Lazy<Mutex<Current>> = Lazy::new(|| {
    Mutex::new(Current {
        release: String::new(),
        artist: String::new(),
        url: String::new(),
    })
});

fn set_current(new: Current) {
    let mut current = CURRENT.lock().unwrap();
    *current = new;
}

fn read_current() -> MutexGuard<'static, Current> {
    CURRENT.lock().unwrap()
}

async fn process_metadata() -> Result<ActivityInfo, String> {
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
        return Ok(ActivityInfo {
            details: String::from("Stopped playback"),
            state: String::new(),
            subtitle: String::new(),
            image: player_name,
        })
    }

    if (playback_status == PlaybackStatus::Paused && show_paused == "false") || playback_status == PlaybackStatus::Stopped {
        return Err("no song is currently playing".to_string());
    }
    
    let metadata = player.get_metadata().expect("could not get metadata");

    let mut rows: Vec<String> = env::var("rows")
        .map_err(|e| e.to_string())?
        .split(",")
        .map(|l| l.to_string())
        .collect();
    
    // Technically, discordipc only lets you change the "details" and "state"
    // but you can add a third row by changing the image alt text, so allow three.
    if rows.len() > 3 {
        rows.truncate(3); // only use the first three
    }
    
    let filter = Regex::new("^.*?\\{([^}]+)\\}.*?$").unwrap(); // fields in config.toml are wrapped in {}
    let fields: Vec<String> = rows.clone()
        .into_iter()
        .map(|l| filter.replace_all(&l, "$1").to_string())
        .collect();

    let mut ret: Vec<String> = Vec::new();
    let mut i: usize = 0;
    
    for field in fields {
        let mut name: String = "xesam:".to_owned();
        name.push_str(&field);

        if let Some(val) = metadata.get(&name) {
            ret.push(rows[i].replace(&format!("{{{field}}}"), &value_to_string(val)));
            i += 1;
        } else {
            return Err(format!("could not get value for {field}, is it a valid xesam field?"));
        }
    }

    while ret.len() < 3 {
        ret.push(String::new());
    }

    if playback_status == PlaybackStatus::Paused && show_paused == "true" {
        player_name.push_str("_paused");
    }

    let current = read_current().clone();

    let mut new = Current {
        release: metadata.album_name().unwrap().to_string(),
        artist: metadata.album_artists().unwrap().join(", "),
        url: current.url.clone(),
    };

    let fetch_cover_art = env::var("fetch_cover_art").map_err(|e| e.to_string())?;
    
    if current != new {
        if fetch_cover_art == "true" {
            if let Ok(url) = get_cover_art(new.clone()).await {
                new.url = url;
            } else {
                new.url = player_name;
            }
        } else {
            new.url = player_name;
        }
        set_current(new.clone());
    }
    
    ret.push(new.url);

    Ok(ActivityInfo {
        details: ret[0].clone(),
        state: ret[1].clone(),
        subtitle: ret[2].clone(),
        image: ret[3].clone(),
    })
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

    loop {
        match process_metadata().await {
            Ok(rows) => {
                let activity = Activity::new()
                    .kind(ActivityType::Listening)
                    .details(rows.details.clone())
                    .state(rows.state.clone())
                    .assets(Assets::new()
                        .large_image(&rows.image, Some(&rows.subtitle)));

                let activity_packet = Packet::new_activity(Some(&activity), None);

                if let Err(why) = client.send_and_wait(activity_packet)?.filter() {
                    eprintln!("couldn't set activity: {why}");
                }
            },
            Err(_) => {
                let _ = client.send_and_wait(Packet::new_activity(None, None))?.filter(); // send a blank packet to clear the rich presence
            }
        }

        thread::sleep(interval);
    }
}