#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::Read;
use std::os::windows::process::CommandExt;
use windows_hotkeys::{HotkeyManager, HotkeyManagerImpl};
use windows_hotkeys::keys::{ModKey, VKey};
use std::sync::{Arc, Mutex};
use std::thread;
use std::process::Command;
use image::{DynamicImage, GenericImageView};
use notify_rust::Notification;
use tray_icon::{TrayIconBuilder, menu::{Menu, MenuEvent, MenuItem, Submenu}, Icon};
use tao::event_loop::{EventLoop, ControlFlow};
use tray_icon::menu::{AboutMetadata, PredefinedMenuItem};
use serde::Deserialize;
use directories::BaseDirs;


#[derive(Deserialize, Debug)]
struct Manifest {
    pub entries: Vec<Entries>,
}

#[derive(Deserialize, Debug)]
struct Entries {
    pub account_name: String,
}

#[derive(Clone, Debug)]
struct Accounts {
    pub active: u8,
    pub accounts: Vec<Account>,
}

impl Accounts {
    fn new() -> Self {
        Accounts {
            active: 0,
            accounts: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
struct Account {
    pub username: String,
    pub button_id: Option<tray_icon::menu::MenuId>,
}

impl Account {
    fn new(username: String) -> Self {

        Account {
            username,
            button_id: None,
        }
    }
}




fn main() {

    let accounts_list: Arc<Mutex<Accounts>> = Arc::new(Mutex::new(read_steamguard_manifest()));

    // Hotkey thread
    let acc_list = accounts_list.clone();
    thread::spawn(move || {
        let acc_list = acc_list.clone();
        let mut hkm = HotkeyManager::new();


        let _ = hkm.register(VKey::F8, &[ModKey::Shift], move || {
            let acc = acc_list.lock().unwrap().clone();
            take_screen(acc.clone()).unwrap();
        });

        Notification::new()
            .summary("Steamguard QR Login")
            .body("Now listening for Shift + F8 to take a screenshot and login to Steamguard!")
            .show()
            .unwrap();

        hkm.event_loop();
    });

    let event_loop = EventLoop::new();

    let tray_menu = Menu::new();

    let main_text = MenuItem::new("Steamguard QR Login", true, None);
    let selected_text = Submenu::new("Selected Account", true);
    let drop_i = MenuItem::new("No account selected", false, None);
    let scan_i = MenuItem::new("Scan", true, None);
    let quit_i = MenuItem::new("Quit", true, None);


    let mut acc = accounts_list.lock().unwrap();
    for a in &mut acc.accounts {
        let b = MenuItem::new(a.username.clone(), true, None);
        a.button_id = Some(b.id().clone());
        selected_text.append(&b).unwrap();
    }
    drop(acc);

    tray_menu.append_items(&[
        &main_text,
        &PredefinedMenuItem::separator(),
        &selected_text,
        &drop_i,
        &scan_i,
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::about(
            None,
            Some(AboutMetadata {
                name: Some("Steamguard QR Login".to_string()),
                copyright: Some("Copyright D3XX3R".to_string()),
                ..Default::default()
            }),
        ),
        &quit_i,
    ]).unwrap();

    let mut _tray_icon = None;

    let menu_channel = MenuEvent::receiver();

    event_loop.run(move |event, _, control_flow| {

        *control_flow = ControlFlow::WaitUntil(
            std::time::Instant::now() + std::time::Duration::from_millis(250)
        );

        if let tao::event::Event::NewEvents(tao::event::StartCause::Init) = event {
            let icon_bytes = include_bytes!("../res/icon.ico");
            let icon_meta = image::load_from_memory(icon_bytes).unwrap();
            let icon = icon_meta.to_rgba8().into_vec();
            let (icon_width, icon_height) = icon_meta.dimensions();

            _tray_icon = Some(
                TrayIconBuilder::new()
                     .with_menu(Box::new(tray_menu.clone()))
                     .with_title("Steamguard QR Login")
                     .with_tooltip("Steamguard QR Login")
                     .with_icon(Icon::from_rgba(icon, icon_width, icon_height).unwrap())
                     .build()
                     .unwrap(),
            );
        }

        if let Ok(event) = menu_channel.try_recv() {
            //println!("{event:?}");
            if event.id == main_text.id() {
                webbrowser::open("https://github.com/d3xx3r/steamguard-qr-login").unwrap();
            } else if event.id == scan_i.id() {
                let acc = accounts_list.lock().unwrap();
                take_screen(acc.clone()).unwrap();
            } else if event.id == quit_i.id() {
                *control_flow = ControlFlow::Exit;
            } else {
                let mut acc_a = accounts_list.lock().unwrap();
                let lmao = acc_a.accounts
                    .iter()
                    .position(|r| r.button_id == Some(event.id.clone()));
                if let Some(a) = lmao {
                    drop_i.set_text(acc_a.accounts[a].clone().username);
                    acc_a.active = a as u8;
                }

            }
        }
    });
}

// take a screenshot of every monitor, read for any QR code and send it to steamguard
fn take_screen(acc_list: Accounts) -> Result<(), Box<dyn std::error::Error>> {
    let screens = xcap::Monitor::all()?;
    let amount = screens.len();
    let mut count = 0;
    for screen in screens.iter() {
        let image = screen.capture_image()?;

        let image = DynamicImage::ImageRgba8(image).into_luma8();

        let read = match read_qr(image) {
            Ok(read) => read,
            Err(_e) => {
                count += 1;
                continue;
            }

        };
        let _ = send_to_steamguard(read, acc_list.clone());
    }
    if count == amount {
        Notification::new()
            .summary("Steamguard QR Login")
            .body("No QR Codes found on screen!")
            .show()
            .unwrap();
    }
    Ok(())
}

// read qr code from image
fn read_qr(img: image::GrayImage) -> Result<String, Box<dyn std::error::Error>> {
    let mut img = rqrr::PreparedImage::prepare(img);

    let grids = img.detect_grids();
    if grids.is_empty() {
        return Err("No QR code found".into());
    }

    let (_meta, content) = grids[0].decode().unwrap();

    if content.contains("https://s.team/q/") {
        Ok(content)
    } else {
        return Err("No valid QR code found".into());
    }


}

// send qr code to steamguard
fn send_to_steamguard(code: String, acc: Accounts) -> Result<(), Box<dyn std::error::Error>> {
    let sel = acc.active as usize;
    let selected = acc.accounts[sel].to_owned().username;

    match Command::new("steamguard")
        .arg("-u")
        .arg(&selected)
        .arg("qr-login")
        .arg("--url")
        .arg(code)
        .creation_flags(0x08000000)
        .output() {
        Ok(a) => a,
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                //println!("Steamguard not found");
                Notification::new()
                    .summary("Steamguard QR Login")
                    .body("Steamguard not found!")
                    .show().unwrap();
            }
            return Ok(());
        }
    };
    //println!("Steamguard: {:?}", String::from_utf8_lossy(&a.stdout));
    Notification::new()
        .summary("Steamguard QR Login")
        .body(&format!("Logged in using: {selected}!"))
        .show()
        .unwrap();

    Ok(())
}

// read manifest from steamguard to get accounts
fn read_steamguard_manifest() -> Accounts{
    if let Some(dir) = BaseDirs::new() {
        let a = dir.data_dir();
        let path = format!("{}{}", a.to_str().unwrap(), "\\steamguard-cli\\maFiles\\manifest.json");

        let mut file = std::fs::File::open(path).unwrap();
        let mut data = String::new();
        file.read_to_string(&mut data).unwrap();

        let data: Manifest = serde_json::from_str(&data).expect("JSON was not well-formatted");

        let mut accounts: Accounts = Accounts::new();

        for entry in data.entries.iter() {
            let a = Account::new(entry.account_name.clone());
            accounts.accounts.push(a)
        }


        accounts
    } else {
        panic!("Could not read manifest!")
    }
}
