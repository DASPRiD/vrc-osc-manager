use image::Rgba;
use ksni::{Icon, MenuItem};

const STANDARD_ICON: &[u8] = include_bytes!("../../assets/icon.png");
const ACTIVE_ICON: &[u8] = include_bytes!("../../assets/icon-active.png");

fn convert(img: &[u8]) -> Result<Vec<u8>, image::ImageError> {
    let img = image::load_from_memory(img)?;
    let mut img = img.to_rgba8();

    for Rgba(pixel) in img.pixels_mut() {
        *pixel = u32::from_be_bytes(*pixel).rotate_right(8).to_be_bytes();
    }

    Ok(img.into_raw())
}

struct KsniTray {
    running: bool,
}

impl ksni::Tray for KsniTray {
    fn id(&self) -> String {
        "vrc-osc-manager".to_string()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        vec![Icon {
            width: 64,
            height: 64,
            data: convert(if self.running {
                ACTIVE_ICON
            } else {
                STANDARD_ICON
            })
            .unwrap(),
        }]
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        use ksni::menu::*;

        vec![StandardItem {
            label: "Exit".into(),
            activate: Box::new(|_| std::process::exit(0)),
            ..Default::default()
        }
        .into()]
    }
}

pub struct Tray {
    handle: ksni::Handle<KsniTray>,
}

impl Tray {
    pub fn new() -> Self {
        let service = ksni::TrayService::new(KsniTray { running: false });
        let handle = service.handle();
        service.spawn();

        Self { handle }
    }

    pub fn set_running(&mut self, running: bool) {
        self.handle.update(|tray: &mut KsniTray| {
            tray.running = running;
        });
    }
}
