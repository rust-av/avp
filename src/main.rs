#[macro_use]
extern crate clap;

extern crate av_data as data;
extern crate av_codec as codec;
extern crate av_format as format;
extern crate matroska;
extern crate libvpx as vpx;

extern crate sdl2;

use clap::Arg;

use sdl2::pixels::PixelFormatEnum;
// use sdl2::rect::Rect;
use sdl2::keyboard::Keycode;

use format::demuxer::*;
use format::stream::*;
use data::packet::*;

// use matroska::demuxer::MKV_DESC;
use matroska::demuxer::MkvDemuxer;

fn playback() {
    use sdl2::event::Event as SDLEvent;
    let sdl_context = sdl2::init().unwrap();
    let video_subsystem = sdl_context.video().unwrap();
    let w = 800usize;
    let h = 600usize;

    let window = video_subsystem.window("rust-sdl2 demo: Video", w as u32, h as u32)
        .position_centered()
        .opengl()
        .build()
        .unwrap();

    let mut canvas = window.into_canvas().build().unwrap();
    let texture_creator = canvas.texture_creator();

    let mut texture = texture_creator.create_texture_streaming(
        PixelFormatEnum::IYUV, w as u32, h as u32).unwrap();
    // Create a U-V gradient
    texture.with_lock(None, |buffer: &mut [u8], pitch: usize| {
        // `pitch` is the width of the Y component
        // The U and V components are half the width and height of Y

        // Set Y (constant)
        for y in 0..h {
            for x in 0..w {
                let offset = y*pitch + x;
                buffer[offset] = 128;
            }
        }

        let y_size = pitch*h;

        // Set U and V (X and Y)
        for y in 0..h/2 {
            for x in 0..w/2 {
                let u_offset = y_size + y*pitch/2 + x;
                let v_offset = y_size + (pitch/2 * h/2) + y*pitch/2 + x;
                buffer[u_offset] = (x*2) as u8;
                buffer[v_offset] = (y*2) as u8;
            }
        }
    }).unwrap();

    canvas.clear();
    canvas.copy(&texture, None, None).unwrap();
    canvas.present();

    let mut event_pump = sdl_context.event_pump().unwrap();

    'running: loop {
        for event in event_pump.poll_iter() {
            match event {
                SDLEvent::Quit {..} | SDLEvent::KeyDown { keycode: Some(Keycode::Escape), .. } => {
                    break 'running
                },
                _ => {}
            }
        }
        // The rest of the game loop goes here...
    }
}

use std::fs::File;
use format::buffer::AccReader;
use std::io::BufReader;

use codec::decoder::Context as DecContext;
use codec::decoder::Codecs as DecCodecs;
use codec::common::CodecList;
use data::frame::ArcFrame;

use vpx::decoder::VP9_DESCR;
// use opus::decoder::OPUS_DESCR;

use std::collections::HashMap;

struct PlaybackContext {
    decoders: HashMap<isize, DecContext>,
    demuxer: Context,
}

impl PlaybackContext {
    fn from_path(s: &str) -> Self {
        let r = File::open(s).unwrap();
        // Context::from_read(demuxers, r).unwrap();
        let ar = AccReader::with_capacity(4 * 1024, BufReader::new(r));

        let mut c = Context::new(Box::new(MkvDemuxer::new()), Box::new(ar));

        c.read_headers().expect("Cannot parse the format headers");

        let decoders = DecCodecs::from_list(&[VP9_DESCR]);

        let mut decs: HashMap<isize, DecContext> = HashMap::with_capacity(2);
        for st in &c.info.streams {
            if let Some(ref codec_id) = st.params.codec_id {
                if let Some(mut ctx) = DecContext::by_name(&decoders, codec_id) {
                    if let Some(ref extradata) = st.params.extradata {
                        ctx.set_extradata(extradata);
                    }
                    ctx.configure().expect("Codec configure failed");
                    decs.insert(st.index as isize, ctx);
                }
            }
        }

        PlaybackContext {
            decoders: decs,
            demuxer: c
        }
    }

    fn decode_one(&mut self) -> Option<ArcFrame> {
        let ref mut c = self.demuxer;
        let ref mut decs = self.decoders;
        if let Ok(event) = c.read_event() {
            match event {
                Event::NewPacket(pkt) => {
                    if let Some(dec) = decs.get_mut(&pkt.stream_index) {
                        println!("Decoding packet at index {}", pkt.stream_index);
                        dec.send_packet(&pkt);
                        dec.receive_frame().ok()
                    } else {
                        println!("Skipping packet at index {}", pkt.stream_index);
                        None
                    }
                },
                _ => {
                    println!("Unsupported event {:?}", event);
                    unimplemented!();
                }
            }
        } else {
            None
        }
    }
}

fn main() {
    let i = Arg::with_name("input")
        .takes_value(true)
        .value_name("INPUT")
        .short("i")
        .index(1)
        .multiple(true);

    let m = app_from_crate!()
        .arg(i)
        .get_matches();

    println!("{:?}", m);

    if let Some(input) = m.value_of("input") {
        let mut ctx = PlaybackContext::from_path(input);
        for i in 0..5 {
            println!("Decoded {:?}", ctx.decode_one());
        }
        // playback(ctx);
    } else {

    }
}
