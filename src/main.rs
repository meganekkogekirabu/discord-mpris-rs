use discordipc::{activity::{Activity, ActivityType, Assets}, Client, packet::Packet};
use dotenv::dotenv;
use mpris::{MetadataValue, PlayerFinder, PlaybackStatus};
use regex::Regex;   
use std::{env, thread};
use std::time::Duration;

struct ActivityInfo {
    details: String,  // 1st row
    state: String,    // 2nd row
    subtitle: String, // 3rd row
    player: String,   // name used to find the icon
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

fn process_metadata() -> Result<ActivityInfo, String> {
    let ignore: Vec<String> = env::var("ignored_players")
        .map_err(|e| e.to_string())?
        .split(",")
        .map(|l| l.to_string())
        .collect();

    let show_paused = env::var("show_paused").map_err(|e| e.to_string())?;

    let mut players = PlayerFinder::new()
        .expect("could not connect to D-Bus")
        .find_all()
        .map_err(|e| e.to_string())?;

    players.retain(|p| !ignore.contains(&p.identity().to_string()));

    let player = &players[0]; // just get the first one, since with .find_active(), players can't be ignored

    let playback_status = player.get_playback_status().map_err(|e| e.to_string())?;

    if (playback_status == PlaybackStatus::Paused && show_paused == "false") || playback_status == PlaybackStatus::Stopped {
        return Err("no song is currently playing".to_string());
    }
    
    let metadata = player.get_metadata().expect("could not get metadata");

    let mut rows: Vec<String> = env::var("rows")
        .map_err(|e| e.to_string())?
        .split(",")
        .map(|l| l.to_string())
        .collect();
    
    // Technically, discordipc only lets you change the "details" and "state" but
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
    
    let mut player = player.identity().to_string().to_lowercase();

    if playback_status == PlaybackStatus::Paused && show_paused == "true" {
        player.push_str("_paused");
    }

    ret.push(player);

    Ok(ActivityInfo {
        details: ret[0].clone(),
        state: ret[1].clone(),
        subtitle: ret[2].clone(),
        player: ret[3].clone(),
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();

    let interval = Duration::from_millis(
        env::var("update_interval")?.parse()
            .expect("update_interval must be a valid integer")
    );

    let client = Client::new_simple("1389363158874980473");
    client.connect_and_wait()?.filter()?;

    loop {
        match process_metadata() {
            Ok(rows) => {
                let activity = Activity::new()
                    .kind(ActivityType::Listening)
                    .details(rows.details.clone())
                    .state(rows.state.clone())
                    .assets(Assets::new()
                        .large_image(&rows.player, Some(&rows.subtitle)));

                let activity_packet = Packet::new_activity(Some(&activity), None);

                if let Err(why) = client.send_and_wait(activity_packet)?.filter() {
                    panic!("couldn't set activity: {why}");
                }
            },
            Err(_) => {
                let _ = client.send_and_wait(Packet::new_activity(None, None))?.filter(); // send a blank packet to clear the rich presence
            }
        }

        thread::sleep(interval);
    }
}