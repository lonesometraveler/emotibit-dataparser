#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use anyhow::Result;
use emotibit_data::{csv::StringRecord, parser, types::DataPacket, writer};
use std::{collections::HashSet, fs::File, io::Write, path::PathBuf};
use tauri::{api::dialog, CustomMenuItem, Menu, Submenu};

fn main() {
    let context = tauri::generate_context!();
    tauri::Builder::default()
        .menu(
            Menu::new().add_submenu(Submenu::new(
                "File",
                Menu::new()
                    .add_item(CustomMenuItem::new("open", "Open").accelerator("cmdOrControl+O"))
                    .add_item(CustomMenuItem::new("close", "Close").accelerator("cmdOrControl+Q")),
            )),
        )
        .on_menu_event(|event| match event.menu_item_id() {
            "open" => {
                dialog::FileDialogBuilder::default()
                    .add_filter("CSV", &["csv"])
                    .pick_file(move |path_buf| match read_write(&path_buf) {
                        Ok(()) => {
                            println!("finished without errors");
                            event.window().close().unwrap();
                        }
                        Err(e) => println!("Error: {:?}", e),
                    });
            }
            "close" => {
                event.window().close().unwrap();
            }
            _ => {}
        })
        .run(context)
        .expect("error while running tauri application");
}

fn read_write(path_buf: &Option<PathBuf>) -> Result<()> {
    let filename: &str = path_buf
        .as_ref()
        .and_then(|name| name.file_stem())
        .and_then(|name| name.to_str())
        .unwrap_or("default");

    let (datapackets, errors): (Vec<_>, Vec<_>) = parser::get_packets(
        &path_buf
            .as_ref()
            .unwrap()
            .clone()
            .into_os_string()
            .into_string()
            .unwrap(),
    )?
    .into_iter()
    .partition(Result::is_ok);

    let mut output_file = path_buf.clone().unwrap();
    output_file.set_file_name(format!("{}_ERROR.csv", filename));

    // Write Errors
    let mut output = File::create(output_file.clone())?;
    for err in errors.into_iter().filter_map(|result| result.err()) {
        writeln!(output, "{}", err)?;
    }

    // Write TimeSyncs
    output_file.set_file_name(format!("{}_timesyncs.csv", filename));
    let mut writer = writer::WriterBuilder::new().from_path(output_file.to_str().unwrap())?;
    match parser::find_syncs(&datapackets) {
        Ok(syncs) => {
            let header =
                StringRecord::from(vec!["RD", "TS_received", "TS_sent", "AK", "RoundTrip"]);
            writer.write(&header)?;
            for packet in syncs {
                writer.write(&packet)?;
            }
        }
        Err(e) => {
            writer.write(&StringRecord::from(vec![format!("{:?}", e)]))?;
        }
    }

    // Extract TypeTags
    let set: HashSet<&str> = HashSet::from_iter(
        datapackets
            .iter()
            .map(|result| result.as_ref().unwrap().data_type.as_str()),
    );

    // Write TimeSyncsMap
    output_file.set_file_name(format!("{}_timeSyncMap.csv", filename));
    let mut writer = writer::WriterBuilder::new().from_path(output_file.to_str().unwrap())?;
    let syncmap = parser::generate_sync_map(&datapackets);
    match &syncmap {
        Ok(map) => {
            let header = StringRecord::from(vec![
                "TE0",
                "TE1",
                "TL0",
                "TL1",
                "TimeSyncsReceived",
                "EmotiBitStartTime",
                "EmotiBitEndTime",
                "DataParserVersion",
            ]);
            writer.write(&header)?;
            writer.write(map)?;
        }
        Err(e) => {
            writer.write(&StringRecord::from(vec![format!("{:?}", e)]))?;
        }
    }

    // Write Packets
    let packets: Vec<DataPacket> = match syncmap {
        Ok(map) => datapackets
            .into_iter()
            .filter_map(|result| result.ok())
            .map(|p| p.inject_host_timestamp(&map))
            .collect(),
        Err(_) => datapackets
            .into_iter()
            .filter_map(|result| result.ok())
            .collect(),
    };

    for t in set.iter() {
        output_file.set_file_name(format!("{}_{}.csv", filename, t));
        let mut writer = writer::WriterBuilder::new().from_path(output_file.to_str().unwrap())?;
        let header = StringRecord::from(vec![
            "LocalTimestamp",
            "EmotiBitTimestamp",
            "PacketNumber",
            "DataLength",
            "TypeTag",
            "ProtocolVersion",
            "DataReliability",
            t,
        ]);
        writer.write(&header)?;
        for packet in packets.iter().filter(|x| x.data_type.as_str() == *t) {
            writer.write(packet)?;
        }
    }

    Ok(())
}
