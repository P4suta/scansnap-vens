//! Contract tests for the directory sink on a real temporary directory.

use std::{
    env, fs, io,
    path::PathBuf,
    process,
    time::{SystemTime, UNIX_EPOCH},
};

use vens_core::{jpeg::JpegError, scan::Side};
use vens_net::{
    host::{DirectorySink, SinkError},
    scan::PageSink,
};

const IMAGE: [u8; 21] = [
    0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x29, 0x50, 0x0A, 0x20, 0x01, 0x01, 0x11, 0x00, 0xFF,
    0xDA, 0x00, 0x02, 0xFF, 0xD9,
];
const HEIGHT_OFFSET: usize = 7;

struct Scratch(PathBuf);

impl Scratch {
    fn create(name: &str) -> io::Result<Self> {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let path = env::temp_dir().join(format!("vens-net-{name}-{}-{unique}", process::id()));
        fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn corrected() -> Vec<u8> {
    let mut image = IMAGE.to_vec();
    image
        .iter_mut()
        .skip(HEIGHT_OFFSET)
        .zip([0x26, 0x80])
        .for_each(|(slot, byte)| *slot = byte);
    image
}

fn written(name: &str) -> io::Result<(Vec<String>, Vec<u8>, Vec<u8>)> {
    let scratch = Scratch::create(name)?;
    let mut sink = DirectorySink::new(scratch.0.clone());
    let (head, tail) = IMAGE.split_at(9);
    let outcome = sink
        .append(0, Side::Front, head)
        .and_then(|()| sink.append(0, Side::Front, tail))
        .and_then(|()| sink.finish(0, Side::Front, 9856))
        .and_then(|()| sink.append(0, Side::Back, &IMAGE))
        .and_then(|()| sink.finish(0, Side::Back, 9856));
    if outcome.is_err() {
        return Err(io::Error::other("the sink refused a valid image"));
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(&scratch.0)? {
        names.push(entry?.file_name().to_string_lossy().into_owned());
    }
    names.sort();
    let front = fs::read(scratch.0.join("sheet-001-front.jpg"))?;
    let back = fs::read(scratch.0.join("sheet-001-back.jpg"))?;
    Ok((names, front, back))
}

fn refused(name: &str) -> io::Result<(Option<JpegError>, usize)> {
    let scratch = Scratch::create(name)?;
    let mut sink = DirectorySink::new(scratch.0.clone());
    let outcome = sink
        .append(0, Side::Front, b"not an image")
        .and_then(|()| sink.finish(0, Side::Front, 9856));
    let reason = match outcome {
        Err(SinkError::Image(error)) => Some(error),
        Ok(()) | Err(SinkError::Io(_)) => None,
    };
    Ok((reason, fs::read_dir(&scratch.0)?.count()))
}

#[test]
fn each_finished_side_becomes_one_file_with_the_corrected_height() {
    assert_eq!(
        written("sides").ok(),
        Some((
            [
                "sheet-001-back.jpg".to_owned(),
                "sheet-001-front.jpg".to_owned()
            ]
            .to_vec(),
            corrected(),
            corrected(),
        ))
    );
}

#[test]
fn bytes_that_are_not_an_image_leave_no_file() {
    assert_eq!(refused("refusal").ok(), Some((Some(JpegError::NotJpeg), 0)));
}
