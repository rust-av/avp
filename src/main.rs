#[macro_use]
extern crate clap;

extern crate av_data as data;
extern crate av_codec as codec;
extern crate av_format as format;
extern crate matroska;
extern crate libvpx as vpx;
extern crate libopus as opus;

extern crate sdl2;

use clap::Arg;

use sdl2::pixels::PixelFormatEnum;
// use sdl2::rect::Rect;
use sdl2::keyboard::Keycode;
use sdl2::render::Canvas;
use sdl2::video::Window;
use sdl2::EventPump;

use format::demuxer::*;
use data::frame::*;

// use matroska::demuxer::MKV_DESC;
use matroska::demuxer::MkvDemuxer;

struct SDLPlayer {
    canvas: Canvas<Window>,
    event_pump: EventPump,
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

        let event_pump = sdl_context.event_pump().unwrap();

        SDLPlayer {
            canvas: canvas,
            event_pump: event_pump,
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
                           v_plane, v_stride).unwrap();

        self.canvas.clear();
        self.canvas.copy(&texture, None, None).unwrap();
        self.canvas.present();
    }

    fn eventloop(&mut self) -> bool {
        use sdl2::event::Event as SDLEvent;

        for event in self.event_pump.poll_iter() {
            match event {
                SDLEvent::Quit {..} |
                    SDLEvent::KeyDown { keycode: Some(Keycode::Escape), .. } => {
                        return true;
                },
                _ => {
                    return false;
                }
            }
        }
        false
    }
}

use std::fs::File;
use format::buffer::AccReader;
use data::params;

use codec::decoder::Context as DecContext;
use codec::decoder::Codecs as DecCodecs;
use codec::common::CodecList;
use data::frame::ArcFrame;


use vpx::decoder::VP9_DESCR;
use opus::decoder::OPUS_DESCR;

use std::collections::HashMap;

struct PlaybackContext {
    decoders: HashMap<isize, DecContext>,
    demuxer: Context,
    video: Option<params::VideoInfo>,
}

impl PlaybackContext {
    fn from_path(s: &str) -> Self {
        let r = File::open(s).unwrap();
        // Context::from_read(demuxers, r).unwrap();
        let ar = AccReader::with_capacity(4 * 1024, r);

        let mut c = Context::new(Box::new(MkvDemuxer::new()), Box::new(ar));

        c.read_headers().expect("Cannot parse the format headers");

        let decoders = DecCodecs::from_list(&[VP9_DESCR, OPUS_DESCR]);

        let mut video_info = None;
        let mut decs: HashMap<isize, DecContext> = HashMap::with_capacity(2);
        for st in &c.info.streams {
            // TODO stream selection
            if let Some(ref codec_id) = st.params.codec_id {
                if let Some(mut ctx) = DecContext::by_name(&decoders, codec_id) {
                    if let Some(ref extradata) = st.params.extradata {
                        ctx.set_extradata(extradata);
                    }
                    ctx.configure().expect("Codec configure failed");
                    decs.insert(st.index as isize, ctx);
                    if let Some(params::MediaKind::Video(ref info)) = st.params.kind {
                        video_info = Some(info.clone());
                    }
                }
            }
        }

        PlaybackContext {
            decoders: decs,
            demuxer: c,
            video: video_info,
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
                        dec.send_packet(&pkt).unwrap(); // TODO report error
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

    if let Some(input) = m.value_of("input") {
        let (s, r) = mpsc::channel();
        let mut play = PlaybackContext::from_path(input);
        let mut p;

        if let Some(ref video) = play.video {
            p = SDLPlayer::new(video.width, video.height, "avp");
        } else {
            p = SDLPlayer::new(640, 480, "avp");
        }

        thread::spawn(move || {
            while let Ok(data) = play.decode_one() {
                if let Some(frame) = data {
                    println!("Decoded {:?}", frame);
                    s.send(frame).unwrap(); // TODO: manage the error
                }
            }
        });

        while let Ok(f) = r.recv() {
            println!("Got {:?}", f);

            match f.kind {
                MediaKind::Video(_) => {
                    // TODO: support resizing
                    // p = Some(SDLPlayer::new(fmt.width, fmt.height, "avp"))
                    p.blit(&f);
                },
                MediaKind::Audio(_) => {
                    // TODO send here just the video
                }
            }

            thread::sleep(time::Duration::from_millis(200));
            if p.eventloop() {
                return;
            }
        }
        // TODO: close once it finished or not?
        while !p.eventloop() {}
    } else {

    }
}
