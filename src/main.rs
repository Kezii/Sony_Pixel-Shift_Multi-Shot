use clap::Parser;
use exif::read_exif;
use log::info;
use memmap::{Mmap, MmapOptions};
use rayon::prelude::*;
use std::io::BufWriter;

mod exif;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    output_file: String,

    #[arg(short, long, value_parser, num_args = 1.., value_delimiter = ' ')]
    input_files: Vec<String>,
}

fn sequence_to_group_id(t: u32) -> (u32, u32) {
    fn seq2idx(s: u32) -> u32 {
        match s {
            1 => 0,
            0 => 1,
            3 => 2,
            2 => 3,
            _ => unreachable!(),
        }
    }

    let sn = t - 1;
    let id = seq2idx(sn % 4);
    let group_id = seq2idx(sn / 4);
    (group_id, id)
}

fn dng_color_channel(row: u32, col: u32) -> usize {
    // Mirrors make_arq.py:dngcolor(); returns 0..2 (R,G,B) for RGGB pattern
    let v = (0x94949494u32 >> ((((row) << 1 & 14) + ((col) & 1)) << 1)) & 3;
    match v {
        0 => 0, // R
        1 => 1, // G
        2 => 2, // B
        3 => 1, // (shouldn't happen for this pattern, but treat as G)
        _ => unreachable!(),
    }
}

#[derive(Debug)]
struct RawImage<'a> {
    _path: String,
    width: u32,
    height: u32,
    _sequence_number: u32,
    group: u32, // which group of 4 images this image belongs to, every group has 4 images
    id_in_group: u32, // which image in the group this image is
    _mmap: Mmap,
    data_pixels: &'a [u16],
    exif: exif::ExifData,
}

impl<'a> RawImage<'a> {
    fn new(path: &str) -> Self {
        let exif = read_exif(path);

        let file = std::fs::File::open(path).unwrap();
        let data = unsafe {
            MmapOptions::new()
                .offset(exif.offset as u64)
                .map(&file)
                .unwrap()
        };

        let data_slice_u16 =
            unsafe { std::slice::from_raw_parts(data.as_ptr() as *const u16, data.len() / 2) };

        let gi = sequence_to_group_id(exif.sequence_number);

        Self {
            _path: path.to_string(),
            width: exif.width,
            height: exif.height,
            _sequence_number: exif.sequence_number,
            group: gi.0,
            id_in_group: gi.1,
            _mmap: data,
            data_pixels: data_slice_u16,
            exif,
        }
    }

    fn get_pixel(&self, x: u32, y: u32) -> u16 {
        let offset = (y * self.width + x) as usize;

        if offset >= self.data_pixels.len() {
            return 0;
        }

        self.data_pixels[offset]
    }

    fn inter_group_offsets(&self) -> (u32, u32) {
        match self.id_in_group {
            0 => (1, 1),
            1 => (0, 1),
            2 => (0, 0),
            3 => (1, 0),
            _ => unreachable!(),
        }
    }
}

fn fill_sony_frame_dng_rgb(
    out: &mut [u16],
    out_width: u32,
    out_height: u32,
    file: &RawImage,
    idx: u32,
    factor: u32,
) {
    let (r_off, c_off) = file.inter_group_offsets();
    let group = idx / 4;
    let rowstart = if group >= 2 { 1 } else { 0 };
    let colstart = if group % 2 == 1 { 1 } else { 0 };

    for row in 0..file.height {
        for col in 0..file.width {
            let rr = (row + r_off) * factor + rowstart;
            let cc = (col + c_off) * factor + colstart;
            if rr >= out_height || cc >= out_width {
                continue;
            }
            let ch = dng_color_channel(row, col);
            let v = file.get_pixel(col, row);
            let base = ((rr * out_width + cc) * 3) as usize;
            out[base + ch] = v;
        }
    }
}

fn main() {
    env_logger::init_from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    );

    let now = std::time::Instant::now();

    let args = Args::parse();

    info!("loading files");
    let mut files = args
        .input_files
        .par_iter()
        .map(|path| RawImage::new(path))
        .collect::<Vec<_>>();

    files.sort_by_key(|file| (file.group, file.id_in_group));

    for file in &files {
        info!(
            "{}: {}x{} ({}mpx), group {}, id {}",
            file._path,
            file.width,
            file.height,
            file.data_pixels.len() / 1000 / 1000,
            file.group,
            file.id_in_group
        );
    }

    if files
        .iter()
        .enumerate()
        .any(|(i, file)| file.id_in_group + file.group * 4 != i as u32)
    {
        panic!("some files are missing");
    }

    // make_arq's out16.ARQ is a TIFF/DNG "Linear Raw" with 3 channels (RGB16),
    // where each pixel stores only one of (R,G,B) depending on Bayer position.
    let (out_width, out_height, factor) = match files.len() {
        4 => (files[0].width, files[0].height, 1u32),
        16 => (files[0].width * 2, files[0].height * 2, 2u32),
        _ => panic!("unsupported number of files (expected 4 or 16)"),
    };

    info!("creating DNG-style RGB buffer {}x{}", out_width, out_height);
    let mut out = vec![0u16; (out_width as usize) * (out_height as usize) * 3];

    info!("placing frame data");
    for (idx, file) in files.iter().enumerate() {
        fill_sony_frame_dng_rgb(&mut out, out_width, out_height, file, idx as u32, factor);
    }

    info!("saving TIFF container (will be tagged as DNG/LinearRaw via exiftool)");
    let f = std::fs::File::create(&args.output_file).unwrap();
    let mut enc = tiff::encoder::TiffEncoder::new(BufWriter::new(f)).unwrap();
    enc.write_image::<tiff::encoder::colortype::RGB16>(out_width, out_height, &out)
        .unwrap();

    info!("applying DNG/ARQ tags via exiftool");
    let src = &files[0].exif;
    let mut cmd = std::process::Command::new("exiftool");
    cmd.arg("-overwrite_original");
    cmd.arg(format!("-Software={}", "psmsmerge"));
    if !src.make.is_empty() {
        cmd.arg(format!("-Make={}", src.make));
    }
    if !src.model.is_empty() {
        cmd.arg(format!("-Model={}", src.model));
    }
    cmd.arg("-SamplesPerPixel=3");
    cmd.arg("-BitsPerSample=16 16 16");
    // TIFF PhotometricInterpretation = 34892 (0x884C) is "Linear Raw"
    cmd.arg("-PhotometricInterpretation=34892");
    cmd.arg("-DNGVersion=1.1.0.0");
    if let Some(wb) = src.wb_rggb_levels {
        cmd.arg(format!(
            "-WB_RGGBLevels={} {} {} {}",
            wb[0], wb[1], wb[2], wb[3]
        ));
    }
    if let Some(black) = src.black_level {
        cmd.arg(format!(
            "-BlackLevel={} {} {} {}",
            black[0], black[1], black[2], black[3]
        ));
    }
    if let Some(white) = src.white_level {
        cmd.arg(format!(
            "-WhiteLevel={} {} {} {}",
            white[0], white[1], white[2], white[3]
        ));
    }
    cmd.arg(&args.output_file);
    let outp = cmd
        .output()
        .expect("failed to execute exiftool for tagging");
    if !outp.status.success() {
        panic!("exiftool failed: {}", String::from_utf8_lossy(&outp.stderr));
    }

    info!("done in {:?}", now.elapsed());
}
