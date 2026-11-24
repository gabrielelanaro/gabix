#![no_std]
#![no_main]

mod arm_tcp_common;
mod arm_v3d;
use arm_tcp_common::*;

const SIDE: usize = 64;
const NAME: &[u8] = b"Gabix V3D triangle";
const VERSION: &[u8; 12] = b"RFB 003.008\n";
const NATIVE: [u8; 16] = [32, 24, 0, 1, 0, 255, 0, 255, 0, 255, 16, 8, 0, 0, 0, 0];
const MODEL: arm_v3d::Triangle = [[32, 8], [8, 56], [56, 56]];
const BACKGROUND: u32 = 0xff194296;
const GOLD: u32 = 0xffffcd28;

struct Stream {
    fd: u32,
    output: [u8; 536], // One payload for the current Pi TCP sender.
    used: usize,
}

impl Stream {
    fn read(&mut self, mut bytes: &mut [u8]) -> Result<(), ()> {
        self.flush()?;
        while !bytes.is_empty() {
            let n = call(
                SYS_READ,
                self.fd,
                bytes.as_mut_ptr() as u32,
                bytes.len() as u32,
            );
            if n == 0 || n == ERROR {
                return Err(());
            }
            bytes = &mut bytes[n as usize..];
        }
        Ok(())
    }

    fn write(&mut self, mut bytes: &[u8]) -> Result<(), ()> {
        while !bytes.is_empty() {
            let n = bytes.len().min(self.output.len() - self.used);
            self.output[self.used..self.used + n].copy_from_slice(&bytes[..n]);
            self.used += n;
            bytes = &bytes[n..];
            if self.used == self.output.len() {
                self.flush()?;
            }
        }
        Ok(())
    }

    // Send any pending reply before waiting for the next client message.
    fn flush(&mut self) -> Result<(), ()> {
        let mut bytes = &self.output[..self.used];
        while !bytes.is_empty() {
            let n = call(
                SYS_WRITE,
                self.fd,
                bytes.as_ptr() as u32,
                bytes.len() as u32,
            );
            if n == 0 || n == ERROR {
                return Err(());
            }
            bytes = &bytes[n as usize..];
        }
        self.used = 0;
        Ok(())
    }

    fn take<const N: usize>(&mut self) -> Result<[u8; N], ()> {
        let mut bytes = [0; N];
        self.read(&mut bytes)?;
        Ok(bytes)
    }

    fn skip(&mut self, mut count: usize) -> Result<(), ()> {
        let mut buffer = [0; 128];
        while count > 0 {
            let n = count.min(buffer.len());
            self.read(&mut buffer[..n])?;
            count -= n;
        }
        Ok(())
    }
}

fn short(bytes: &[u8]) -> usize {
    u16::from_be_bytes([bytes[0], bytes[1]]) as usize
}

struct Format {
    size: usize,
    big_endian: bool,
    max: [u32; 3],
    shift: [u8; 3],
}

impl Format {
    fn parse(bytes: &[u8; 16]) -> Result<Self, ()> {
        let bits = bytes[0];
        if !matches!(bits, 8 | 16 | 32) || bytes[1] > bits || bytes[3] == 0 {
            return Err(());
        }
        let max = [
            short(&bytes[4..]) as u32,
            short(&bytes[6..]) as u32,
            short(&bytes[8..]) as u32,
        ];
        let shift = [bytes[10], bytes[11], bytes[12]];
        let mut used = 0;
        for i in 0..3 {
            let width = max[i].count_ones();
            if max[i] == 0
                || !(max[i] + 1).is_power_of_two()
                || width + u32::from(shift[i]) > u32::from(bits)
            {
                return Err(());
            }
            let mask = max[i] << shift[i];
            if used & mask != 0 {
                return Err(());
            }
            used |= mask;
        }
        if used.count_ones() > u32::from(bytes[1]) {
            return Err(());
        }
        Ok(Self {
            size: usize::from(bits / 8),
            big_endian: bytes[2] != 0,
            max,
            shift,
        })
    }

    fn encode(&self, bgra: &[u8], out: &mut [u8]) {
        let mut pixel = 0u32;
        for i in 0..3 {
            let channel = u32::from(bgra[2 - i]);
            pixel |= ((channel * self.max[i] + 127) / 255) << self.shift[i];
        }
        if self.big_endian {
            out.copy_from_slice(&pixel.to_be_bytes()[4 - self.size..]);
        } else {
            out.copy_from_slice(&pixel.to_le_bytes()[..self.size]);
        }
    }
}

// Enlarge each GPU pixel to an 8x8 block without allocating a second framebuffer.
const SCALE: usize = 8;
const DESKTOP: usize = SIDE * SCALE;
type Point = (usize, usize);

#[derive(Copy, Clone)]
struct Rect {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
}

impl Rect {
    fn intersect(self, other: Rect) -> Rect {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        Rect {
            x,
            y,
            w: (self.x + self.w).min(other.x + other.w).saturating_sub(x),
            h: (self.y + self.h).min(other.y + other.h).saturating_sub(y),
        }
    }
    fn wire(self) -> [u8; 8] {
        let mut bytes = [0; 8];
        for (field, n) in bytes
            .chunks_exact_mut(2)
            .zip([self.x, self.y, self.w, self.h])
        {
            field.copy_from_slice(&(n as u16).to_be_bytes());
        }
        bytes
    }
    fn empty(self) -> bool {
        self.w == 0 || self.h == 0
    }
}

fn dot_bounds((x, y): Point) -> Rect {
    let left = x.saturating_sub(5);
    let top = y.saturating_sub(5);
    Rect {
        x: left,
        y: top,
        w: x + 6 - left,
        h: y + 6 - top,
    }
}

struct Scene<'a> {
    frame: &'a mut arm_v3d::Frame,
    pointer: Option<Point>,
}
impl Scene<'_> {
    fn pixel(&self, x: usize, y: usize) -> &[u8] {
        let offset = ((y / SCALE) * SIDE + x / SCALE) * 4;
        &self.frame[offset..offset + 4]
    }

    fn raw(&self, stream: &mut Stream, format: &Format, rect: Rect) -> Result<(), ()> {
        let mut row = [0; DESKTOP * 4];
        let stride = rect.w * format.size;
        let mut used = 0;
        for y in rect.y..rect.y + rect.h {
            if used + stride > row.len() {
                stream.write(&row[..used])?;
                used = 0;
            }
            for (i, out) in row[used..used + stride]
                .chunks_exact_mut(format.size)
                .enumerate()
            {
                let x = rect.x + i;
                let d = self
                    .pointer
                    .map(|(cx, cy)| x.abs_diff(cx).pow(2) + y.abs_diff(cy).pow(2));
                let pixel = match d {
                    Some(0..=9) => &[255; 4][..],
                    Some(10..=25) => &[0; 4][..],
                    _ => self.pixel(x, y),
                };
                format.encode(pixel, out);
            }
            used += stride;
        }
        stream.write(&row[..used])
    }

    // RRE paints a background followed by solid rectangles. Derive each run
    // from the GPU output in source coordinates, then clip its scaled bounds.
    fn runs(
        &self,
        rect: Rect,
        mut emit: impl FnMut(&[u8], Rect) -> Result<(), ()>,
    ) -> Result<(), ()> {
        let bg = self.pixel(0, 0);
        for y in rect.y / SCALE..(rect.y + rect.h).div_ceil(SCALE) {
            let mut x = rect.x / SCALE;
            let end = (rect.x + rect.w).div_ceil(SCALE);
            while x < end {
                let start = x;
                let pixel = self.pixel(x * SCALE, y * SCALE);
                x += 1;
                while x < end && self.pixel(x * SCALE, y * SCALE) == pixel {
                    x += 1;
                }
                if pixel != bg {
                    let run = Rect {
                        x: start * SCALE,
                        y: y * SCALE,
                        w: (x - start) * SCALE,
                        h: SCALE,
                    }
                    .intersect(rect);
                    emit(
                        pixel,
                        Rect {
                            x: run.x - rect.x,
                            y: run.y - rect.y,
                            ..run
                        },
                    )?;
                }
            }
        }
        Ok(())
    }

    fn rre(&self, stream: &mut Stream, format: &Format, rect: Rect) -> Result<(), ()> {
        let mut count = 0u32;
        self.runs(rect, |_, _| {
            count += 1;
            Ok(())
        })?;
        stream.write(&count.to_be_bytes())?;
        let mut buf = [0; 12];
        format.encode(self.pixel(0, 0), &mut buf[..format.size]);
        stream.write(&buf[..format.size])?;
        self.runs(rect, |pixel, run| {
            format.encode(pixel, &mut buf[..format.size]);
            buf[format.size..format.size + 8].copy_from_slice(&run.wire());
            stream.write(&buf[..format.size + 8])
        })
    }

    fn update(
        &self,
        stream: &mut Stream,
        format: &Format,
        region: Rect,
        full: bool,
        rre: bool,
        old: Option<Point>,
        send_cursor: bool,
    ) -> Result<(), ()> {
        let mut parts = [(region, false); 2];
        let mut count = 0;
        let mut add = |rect: Rect, compressed: bool| {
            let clipped = rect.intersect(region);
            if !clipped.empty() {
                parts[count] = (clipped, compressed);
                count += 1;
            }
        };
        if full {
            add(region, rre);
        } else if let Some(p) = old {
            add(dot_bounds(p), false);
        }
        if !full || rre {
            if let Some(p) = self.pointer {
                add(dot_bounds(p), false);
            }
        }
        stream.write(&[0, 0, 0, count as u8 + u8::from(send_cursor)])?;
        if send_cursor {
            cursor_shape(stream, format)?;
        }
        for &(rect, compressed) in &parts[..count] {
            let mut header = [0; 12];
            header[..8].copy_from_slice(&rect.wire());
            header[11] = if compressed { 2 } else { 0 };
            stream.write(&header)?;
            if compressed {
                self.rre(stream, format, rect)?;
            } else {
                self.raw(stream, format, rect)?;
            }
        }
        Ok(())
    }
}

// RFC 6143 Cursor pseudo-rectangle. TigerVNC positions this sprite locally,
// so moving the pointer does not wait for a Wi-Fi round trip.
fn cursor_shape(stream: &mut Stream, format: &Format) -> Result<(), ()> {
    let mut data = [0u8; 12 + 11 * 11 * 4 + 22];
    data[..8].copy_from_slice(
        &Rect {
            x: 5,
            y: 5,
            w: 11,
            h: 11,
        }
        .wire(),
    );
    data[8..12].copy_from_slice(&(-239i32).to_be_bytes());
    let mask = 12 + 121 * format.size;
    for y in 0usize..11 {
        for x in 0usize..11 {
            let d = x.abs_diff(5).pow(2) + y.abs_diff(5).pow(2);
            let at = 12 + (y * 11 + x) * format.size;
            format.encode(
                if d <= 9 { &[255; 4] } else { &[0; 4] },
                &mut data[at..at + format.size],
            );
            if d <= 25 {
                data[mask + y * 2 + x / 8] |= 0x80 >> (x % 8);
            }
        }
    }
    stream.write(&data[..mask + 22])
}

fn serve(stream: &mut Stream) -> Result<(), ()> {
    stream.write(VERSION)?;
    if &stream.take::<12>()? != VERSION {
        return Err(());
    }
    stream.write(&[1, 1])?; // one offered security type: None
    if stream.take::<1>()? != [1] {
        return Err(());
    }
    stream.write(&[0; 4])?;
    stream.take::<1>()?; // ClientInit shared flag
    stream.write(&(DESKTOP as u16).to_be_bytes())?;
    stream.write(&(DESKTOP as u16).to_be_bytes())?;
    stream.write(&NATIVE)?;
    stream.write(&(NAME.len() as u32).to_be_bytes())?;
    stream.write(NAME)?;

    let mut frame = [0; SIDE * SIDE * 4];
    arm_v3d::draw(&mut frame, &MODEL, BACKGROUND, GOLD)?;
    let mut format = Format::parse(&NATIVE)?;
    let mut fresh = true;
    let mut scene = Scene {
        frame: &mut frame,
        pointer: None,
    };
    let bounds = Rect {
        x: 0,
        y: 0,
        w: DESKTOP,
        h: DESKTOP,
    };
    let mut translation = (0i16, 0i16);
    let mut grab_offset = None;
    let mut primary_down = false;
    let mut shown = None;
    let mut pending = None;
    let mut rre = false;
    let mut local_cursor = false;
    let mut send_cursor = false;
    loop {
        match stream.take::<1>()?[0] {
            0 => {
                stream.skip(3)?;
                format = Format::parse(&stream.take()?)?;
                send_cursor = local_cursor;
                fresh = true;
            }
            2 => {
                let head = stream.take::<3>()?;
                rre = false;
                let was_local = local_cursor;
                local_cursor = false;
                for _ in 0..short(&head[1..]) {
                    match i32::from_be_bytes(stream.take()?) {
                        2 => rre = true,
                        -239 => local_cursor = true,
                        _ => {}
                    }
                }
                if was_local != local_cursor {
                    fresh = true;
                    send_cursor = local_cursor;
                    if local_cursor {
                        scene.pointer = None;
                    }
                }
            }
            3 => {
                let request = stream.take::<9>()?;
                let region = Rect {
                    x: short(&request[1..]),
                    y: short(&request[3..]),
                    w: short(&request[5..]),
                    h: short(&request[7..]),
                }
                .intersect(bounds);
                pending = Some((region, fresh || request[0] == 0));
            }
            4 => stream.skip(7)?, // KeyEvent
            5 => {
                let motion = stream.take::<5>()?;
                let point = (
                    short(&motion[1..]).min(DESKTOP - 1),
                    short(&motion[3..]).min(DESKTOP - 1),
                );
                if !local_cursor {
                    scene.pointer = Some(point);
                }
                let down = motion[0] & 1 != 0;
                let gpu = ((point.0 / SCALE) as i16, (point.1 / SCALE) as i16);
                if down && !primary_down && scene.pixel(point.0, point.1) == GOLD.to_le_bytes() {
                    grab_offset = Some((gpu.0 - translation.0, gpu.1 - translation.1));
                }
                if let Some((gx, gy)) = grab_offset {
                    let next = (gpu.0 - gx, gpu.1 - gy);
                    if next != translation {
                        let points = MODEL.map(|[x, y]| [x + next.0, y + next.1]);
                        arm_v3d::draw(scene.frame, &points, BACKGROUND, GOLD)?;
                        translation = next;
                        fresh = true;
                    }
                }
                if !down {
                    grab_offset = None;
                }
                primary_down = down;
            }
            6 => {
                stream.skip(3)?;
                let len = u32::from_be_bytes(stream.take()?);
                if len > 1 << 20 {
                    return Err(());
                }
                stream.skip(len as usize)?;
            }
            _ => return Err(()),
        }
        if let Some((region, full)) = pending {
            if full || fresh || send_cursor || scene.pointer != shown {
                scene.update(
                    stream,
                    &format,
                    region,
                    full || fresh,
                    rre,
                    shown,
                    send_cursor,
                )?;
                send_cursor = false;
                pending = None;
                fresh = false;
                shown = scene.pointer;
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let (listener, fd) = accept(5900);
    let _ = serve(&mut Stream {
        fd,
        output: [0; 536],
        used: 0,
    });
    finish(listener, fd)
}
