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
use sdl2::{ AudioSubsystem, VideoSubsystem, EventPump };
use sdl2::audio::{ AudioQueue, AudioSpecDesired };

use format::demuxer::*;
use data::frame::*;

// use matroska::demuxer::MKV_DESC;
use matroska::demuxer::MkvDemuxer;

fn sdl_setup() -> (AudioSubsystem, VideoSubsystem, EventPump) {
    let sdl_context = sdl2::init().unwrap();
    let video_subsystem = sdl_context.video().unwrap();
    let audio_subsystem = sdl_context.audio().unwrap();
    let event_pump = sdl_context.event_pump().unwrap();

    (audio_subsystem, video_subsystem, event_pump)
}

trait NewCanvas {
    fn new_canvas(&self, w: usize, h: usize, name: &str) -> Canvas<Window>;
}

impl NewCanvas for VideoSubsystem {
    fn new_canvas(&self, w: usize, h: usize, name: &str) -> Canvas<Window> {
        let window = self.window(name, w as u32, h as u32)
            .position_centered()
            .opengl()
            .build()
            .unwrap();
        window.into_canvas().build().unwrap()
    }
}

trait Blit {
    fn blit(&mut self, frame: &Frame);
}

impl Blit for Canvas<Window> {
    fn blit(&mut self, frame: &Frame) {
        let (w, h) = self.window().size();
        let texture_creator = self.texture_creator();

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

        self.clear();
        self.copy(&texture, None, None).unwrap();
        self.present();
    }
}

trait EventLoop {
    fn eventloop(&mut self) -> bool;
}

impl EventLoop for EventPump {
    fn eventloop(&mut self) -> bool {
        use sdl2::event::Event as SDLEvent;

        for event in self.poll_iter() {
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
    audio: Option<params::AudioInfo>,
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
        let mut audio_info = None;
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
                    match st.params.kind {
                        Some(params::MediaKind::Video(ref info)) => {
                            video_info = Some(info.clone());
                        },
                        Some(params::MediaKind::Audio(ref info)) => {
                            audio_info = Some(info.clone());
                        },
                        _ => {},
                    }
                }
            }
        }

        PlaybackContext {
            decoders: decs,
            demuxer: c,
            video: video_info,
            audio: audio_info,
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
        let (audio, video, mut event) = sdl_setup();
/*        let (v_s, v_r) = mpsc::channel();
        let (a_s, a_r) = mpsc::channel(); */
        let (s, r) = mpsc::channel();
        let mut play = PlaybackContext::from_path(input);

        let mut v_out;

        // TODO: on the fly reconfiguration
        if let Some(ref info) = play.video {
            v_out = video.new_canvas(info.width, info.height, "avp");
        } else {
            v_out = video.new_canvas(640, 480, "avp");
        }

        let mut a_out: Option<AudioQueue<i16>> = None;

        if let Some(ref info) = play.audio {
            let desired = AudioSpecDesired {
                freq: Some(info.rate as i32),
                channels: info.map.as_ref().map(|m| m.len() as u8),
                samples: Some(960),
            };
            let mut a = audio.open_queue(None, &desired).unwrap();
            println!("{:?}", a.spec()); // TODO make sure it is correct?
            a_out = Some(a);
        }

        thread::spawn(move || {
            while let Ok(data) = play.decode_one() {
                if let Some(frame) = data {
                    println!("Decoded {:?}", frame);
                    match frame.kind {
                        MediaKind::Video(_) => {
                            s.send(frame).unwrap(); // TODO: manage the error
                        },
                        MediaKind::Audio(_) => {
                            s.send(frame).unwrap();
                        }
                    }
                }
            }
        });


        a_out.as_mut().unwrap().resume();

        while let Ok(frame) = r.recv() {
            println!("Got {:?}", frame);

            match frame.kind {
                MediaKind::Video(_) => {
                    v_out.blit(&frame);
                },
                MediaKind::Audio(ref info) => {
                    let data = frame.buf.as_slice(0).unwrap();
                    a_out.as_mut().unwrap().queue(&data[.. info.samples]);
                }
            }

            // thread::sleep(time::Duration::from_millis(200));
            if event.eventloop() {
                return;
            }
        }

        // TODO: close once it finished or not?
        while !event.eventloop() {}
    } else {

    }
}
