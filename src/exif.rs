use std::process::Command;

#[derive(Debug, Clone)]
pub struct ExifData {
    pub width: u32,
    pub height: u32,
    pub sequence_number: u32,
    pub offset: u32,
    pub make: String,
    pub model: String,
    pub wb_rggb_levels: Option<[u16; 4]>,
    pub black_level: Option<[u16; 4]>,
    pub white_level: Option<[u16; 4]>,
}

fn parse_u16_list(v: &str) -> Vec<u16> {
    v.split_whitespace()
        .filter_map(|s| s.parse::<u16>().ok())
        .collect()
}

fn list_to_4(mut v: Vec<u16>) -> Option<[u16; 4]> {
    if v.is_empty() {
        return None;
    }
    if v.len() == 1 {
        v = vec![v[0], v[0], v[0], v[0]];
    }
    if v.len() != 4 {
        return None;
    }
    Some([v[0], v[1], v[2], v[3]])
}

pub fn read_exif(path: &str) -> ExifData {
    let exifs = Command::new("exiftool")
        .arg(path)
        .output()
        .expect("failed to execute process");

    let exifs = String::from_utf8_lossy(&exifs.stdout);

    let exifs = exifs.lines().map(|line| {
        let mut parts = line.splitn(2, ':');
        let key = parts.next().unwrap().trim().to_string();
        let value = parts.next().unwrap().trim().to_string();
        (key, value)
    });

    let mut exif_data = ExifData {
        width: 0,
        height: 0,
        sequence_number: 0,
        offset: 0,
        make: String::new(),
        model: String::new(),
        wb_rggb_levels: None,
        black_level: None,
        white_level: None,
    };

    for (key, value) in exifs {
        match key.as_str() {
            "Strip Offsets" => exif_data.offset = value.parse::<u32>().unwrap(),
            "Image Width" => exif_data.width = value.parse::<u32>().unwrap(),
            "Image Height" => exif_data.height = value.parse::<u32>().unwrap(),
            "Sequence Number" => exif_data.sequence_number = value.parse::<u32>().unwrap(),
            "Make" => exif_data.make = value,
            "Model" => exif_data.model = value,
            "WB RGGB Levels" | "WB_RGGBLevels" => {
                exif_data.wb_rggb_levels = list_to_4(parse_u16_list(&value))
            }
            "Black Level" | "BlackLevel" => {
                exif_data.black_level = list_to_4(parse_u16_list(&value))
            }
            "White Level" | "WhiteLevel" => {
                exif_data.white_level = list_to_4(parse_u16_list(&value))
            }
            _ => (),
        }
    }

    if exif_data.width == 0
        || exif_data.height == 0
        || exif_data.sequence_number == 0
        || exif_data.offset == 0
    {
        panic!("Failed to read exif data");
    }

    exif_data
}
