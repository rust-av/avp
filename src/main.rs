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
use sdl2::render::Canvas;
use sdl2::video::Window;
use sdl2::Sdl;

use format::demuxer::*;
use data::frame::*;

// use matroska::demuxer::MKV_DESC;
use matroska::demuxer::MkvDemuxer;

struct SDLPlayer {
    canvas: Canvas<Window>,
    context: Sdl
}

impl SDLPlayer {
    fn new(w: usize, h: usize, name: &str) -> Self {
        let sdl_context = sdl2::init().unwrap();
        let video_subsystem = sdl_context.video().unwrap();

        let window = video_subsystem.window(name, w as u32, h as u32)
            .position_centered()
            .opengl()
            .build()
            .unwrap();
        let canvas = window.into_canvas().build().unwrap();

        SDLPlayer {
            canvas: canvas,
            context: sdl_context,
        }
    }

    fn blit(&mut self, frame: &Frame) {
        let (w, h) = self.canvas.window().size();
        let texture_creator = self.canvas.texture_creator();

        let mut texture = texture_creator.create_texture_streaming(
            PixelFormatEnum::IYUV, w, h).unwrap();

        let y_plane = frame.buf.as_slice(0).unwrap();
        let y_stride = frame.buf.linesize(0).unwrap();
        let u_plane = frame.buf.as_slice(1).unwrap();
        let u_stride = frame.buf.linesize(1).unwrap();
        let v_plane = frame.buf.as_slice(2).unwrap();
        let v_stride = frame.buf.linesize(2).unwrap();

        texture.update_yuv(None,
                           y_plane, y_stride,
                           u_plane, u_stride,
                           v_plane, v_stride);

        self.canvas.clear();
        self.canvas.copy(&texture, None, None).unwrap();
        self.canvas.present();
    }

    fn eventloop(&mut self) {
        use sdl2::event::Event as SDLEvent;

        let mut event_pump = self.context.event_pump().unwrap();

        'running: loop {
            for event in event_pump.poll_iter() {
                match event {
                    SDLEvent::Quit {..} |
                        SDLEvent::KeyDown { keycode: Some(Keycode::Escape), .. } => {
                        break 'running
                    },
                    _ => {}
                }
            }
            // The rest of the game loop goes here...
        }
    }
}

use std::fs::File;
use format::buffer::AccReader;

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
        let ar = AccReader::with_capacity(4 * 1024, r);

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

    fn decode_one(&mut self) -> Result<Option<ArcFrame>, String> {
        let ref mut c = self.demuxer;
        let ref mut decs = self.decoders;
        match c.read_event() {
            Ok(event) => match event {
                Event::NewPacket(pkt) => {
                    if let Some(dec) = decs.get_mut(&pkt.stream_index) {
                        println!("Decoding packet at index {}", pkt.stream_index);
                        dec.send_packet(&pkt);
                        Ok(dec.receive_frame().ok())
                    } else {
                        println!("Skipping packet at index {}", pkt.stream_index);
                        Ok(None)
                    }
                },
                _ => {
                    println!("Unsupported event {:?}", event);
                    unimplemented!();
                }
            },
            Err(err) => {
                println!("No more events {:?}", err);
                Err("TBD".to_owned())
            }
        }
    }
}

use std::thread;
use std::sync::mpsc;

use std::time;

use data::frame::MediaKind;

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
        let (s, r) = mpsc::channel();
        let input_path = input.to_owned();
        thread::spawn(move || {
            let mut ctx = PlaybackContext::from_path(&input_path);
            while let Ok(data) = ctx.decode_one() {
                if let Some(frame) = data {
                    println!("Decoded {:?}", frame);
                    s.send(frame);
                }
            }
        });

        let mut p  = None;
        while let Ok(f) = r.recv() {
            println!("Got {:?}", f);
            if p.is_none() {
                if let MediaKind::Video(ref fmt) = f.kind {
                    // TODO: support resizing
                    p = Some(SDLPlayer::new(fmt.width, fmt.height, "avp"))
                }
            }
            p.as_mut().unwrap().blit(&f);
            thread::sleep(time::Duration::from_millis(200));
        }

        p.as_mut().unwrap().eventloop();
    } else {

    }
}
