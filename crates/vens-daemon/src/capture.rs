use std::{
    io::{self, Write},
    path::{Path, PathBuf},
};

use pdf_writer::{Content, Filter, Finish, Name, Rect, Ref};
use vens_core::{
    jpeg::{self, JpegError},
    scan::Side,
};
use vens_net::scan::PageSink;

const SCAN_DPI: f32 = 300.0;
const POINTS_PER_INCH: f32 = 72.0;
const IMAGE_NAME: Name<'_> = Name(b"Scan");
const IMAGE_LIMIT: usize = 256 * 1024 * 1024;
const PAGE_LIMIT: usize = 256;

#[derive(Debug, PartialEq, Eq)]
pub enum CaptureError {
    Image(JpegError),
    Dimensions,
    Color(u8),
    Limit,
}

/// Gathers the corrected sides of a scan as individual pages without re-encoding their images.
pub struct Pdf {
    document: pdf_writer::Pdf,
    pages: Vec<Ref>,
    jpeg: Vec<u8>,
    image_bytes: usize,
}

impl Default for Pdf {
    fn default() -> Self {
        Self {
            document: pdf_writer::Pdf::new(),
            pages: Vec::new(),
            jpeg: Vec::new(),
            image_bytes: 0,
        }
    }
}

impl Pdf {
    /// Writes a finished PDF to the destination, then exposes it under its final name.
    ///
    /// # Errors
    ///
    /// Fails if an image remains unfinished, the document is empty, or the filesystem can't publish it.
    pub fn publish(mut self, folder: &Path) -> io::Result<PathBuf> {
        if self.pages.is_empty() || !self.jpeg.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "the scan didn't finish any document",
            ));
        }
        let count = i32::try_from(self.pages.len()).map_err(io::Error::other)?;
        self.document.catalog(Ref::new(1)).pages(Ref::new(2));
        self.document
            .pages(Ref::new(2))
            .kids(self.pages)
            .count(count);
        let mut stage = tempfile::Builder::new()
            .prefix("scan-")
            .suffix(".part")
            .rand_bytes(12)
            .tempfile_in(folder)?;
        stage.write_all(&self.document.finish())?;
        stage.as_file().sync_all()?;
        let destination = stage.path().with_extension("pdf");
        stage
            .persist_noclobber(&destination)
            .map_err(|error| error.error)?;
        Ok(destination)
    }
}

impl PageSink for Pdf {
    type Error = CaptureError;

    fn append(&mut self, _: u32, _: Side, data: &[u8]) -> Result<(), Self::Error> {
        self.image_bytes = self
            .image_bytes
            .checked_add(data.len())
            .filter(|total| *total <= IMAGE_LIMIT)
            .ok_or(CaptureError::Limit)?;
        self.jpeg.extend_from_slice(data);
        Ok(())
    }

    fn finish(&mut self, _: u32, _: Side, lines: u16) -> Result<(), Self::Error> {
        if self.pages.len() >= PAGE_LIMIT {
            return Err(CaptureError::Limit);
        }
        jpeg::set_height(&mut self.jpeg, lines).map_err(CaptureError::Image)?;
        let frame = jpeg::frame_size(&self.jpeg).map_err(CaptureError::Image)?;
        if frame.width == 0 || frame.height == 0 {
            return Err(CaptureError::Dimensions);
        }
        let components = jpeg::components(&self.jpeg).map_err(CaptureError::Image)?;
        let base = i32::try_from(self.pages.len())
            .ok()
            .and_then(|pages| pages.checked_mul(3))
            .and_then(|index| index.checked_add(3))
            .ok_or(CaptureError::Limit)?;
        let page_ref = Ref::new(base);
        let image_ref = Ref::new(base.saturating_add(1));
        let stream_ref = Ref::new(base.saturating_add(2));

        let mut image = self.document.image_xobject(image_ref, &self.jpeg);
        image.filter(Filter::DctDecode);
        image.width(i32::from(frame.width));
        image.height(i32::from(frame.height));
        image.bits_per_component(8);
        match components {
            1 => image.color_space().device_gray(),
            3 => image.color_space().device_rgb(),
            count => return Err(CaptureError::Color(count)),
        }
        image.finish();
        self.jpeg.clear();

        let w = f32::from(frame.width) * (POINTS_PER_INCH / SCAN_DPI);
        let h = f32::from(frame.height) * (POINTS_PER_INCH / SCAN_DPI);
        let mut content = Content::new();
        content
            .transform([w, 0.0, 0.0, h, 0.0, 0.0])
            .x_object(IMAGE_NAME);
        self.document.stream(stream_ref, &content.finish());
        let mut page = self.document.page(page_ref);
        page.parent(Ref::new(2));
        page.media_box(Rect::new(0.0, 0.0, w, h));
        page.contents(stream_ref);
        page.resources().x_objects().pair(IMAGE_NAME, image_ref);
        page.finish();
        self.pages.push(page_ref);
        Ok(())
    }
}
