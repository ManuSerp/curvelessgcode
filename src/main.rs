use clap::Parser;
use std::{error::Error, f64::consts::TAU, fs, path::{Path, PathBuf}};

const MAX_STEP_ANGLE: f64 = 2.0_f64.to_radians();
const MAX_SAGITTA: f64 = 0.01;
const EPS: f64 = 1e-8;

#[derive(Parser)]
#[command(name = "no-angle-to-call", version, about = "Convert G2/G3 arcs to small G1 moves")]
struct Args {
    /// Only count arcs and predicted G1 moves; do not write a file.
    #[arg(long)]
    check: bool,
    /// Input G-code file.
    file: PathBuf,
}

#[derive(Clone, Copy)]
enum Plane { XY, XZ, YZ }
#[derive(Clone, Copy)]
enum Motion { Other, Cw, Ccw }
#[derive(Default, Clone, Copy)]
struct Pos { x: f64, y: f64, z: f64 }

fn main() {
    if let Err(e) = run() { eprintln!("error: {e}"); std::process::exit(1); }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let input = fs::read_to_string(&args.file)?;
    let (output, arcs, segments) = convert(&input)?;
    if args.check {
        println!("{}: {arcs} arc(s) (G2/G3) -> {segments} G1 segment(s)", args.file.display());
    } else {
        let output_path = output_path(&args.file);
        fs::write(&output_path, output)?;
        println!("Converted {arcs} arc(s) into {segments} G1 segments: {}", output_path.display());
    }
    Ok(())
}

fn output_path(input: &Path) -> PathBuf {
    let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
    let ext = input.extension().and_then(|s| s.to_str());
    let name = match ext { Some(e) => format!("{stem}.no-angle-to-call.{e}"), None => format!("{stem}.no-angle-to-call") };
    input.with_file_name(name)
}

fn convert(text: &str) -> Result<(String, usize, usize), Box<dyn Error>> {
    let mut state = State::default();
    let mut out = String::new();
    let (mut arcs, mut segments) = (0, 0);
    for (line_no, line) in text.lines().enumerate() {
        let result = state.line(line).map_err(|e| format!("line {}: {e}", line_no + 1))?;
        out.push_str(&result.text);
        out.push('\n');
        arcs += result.arcs;
        segments += result.segments;
    }
    if text.is_empty() || !text.ends_with('\n') { out.pop(); }
    Ok((out, arcs, segments))
}

struct State {
    pos: Pos,
    absolute: bool,
    center_absolute: bool,
    plane: Option<Plane>,
    motion: Motion,
}
impl Default for State {
    fn default() -> Self { Self { pos: Pos::default(), absolute: true, center_absolute: false, plane: None, motion: Motion::Other } }
}
struct LineResult { text: String, arcs: usize, segments: usize }

impl State {
    fn line(&mut self, line: &str) -> Result<LineResult, String> {
        let (code, comment) = split_comment(line);
        let words = parse_words(code)?;
        if words.is_empty() { return Ok(LineResult { text: line.to_string(), arcs: 0, segments: 0 }); }
        let mut arc = None;
        for (letter, value) in &words {
            if *letter != 'G' { continue; }
            let g = value.parse::<f64>().map_err(|_| format!("invalid G code {value}"))?;
            if (g - 90.1).abs() < EPS { self.center_absolute = true; }
            else if (g - 91.1).abs() < EPS { self.center_absolute = false; }
            else { match if (g - g.round()).abs() < EPS { g as i32 } else { -1 } {
                0 | 1 => self.motion = Motion::Other,
                2 => { self.motion = Motion::Cw; arc = Some(Motion::Cw); },
                3 => { self.motion = Motion::Ccw; arc = Some(Motion::Ccw); },
                17 => self.plane = Some(Plane::XY),
                18 => self.plane = Some(Plane::XZ),
                19 => self.plane = Some(Plane::YZ),
                90 => self.absolute = true,
                91 => self.absolute = false,
                _ => {}
            }}
        }
        let selected_arc = arc.or(match self.motion { Motion::Cw => Some(Motion::Cw), Motion::Ccw => Some(Motion::Ccw), _ => None });
        if let Some(direction) = selected_arc {
            let plane = self.plane.unwrap_or(Plane::XY);
            let mut converted = self.convert_arc(&words, direction, plane)?;
            let count = converted.len();
            preserve_non_geometry_words(&mut converted, &words);
            return Ok(LineResult { text: format_lines(converted, comment), arcs: 1, segments: count });
        }
        self.update_pos(&words);
        Ok(LineResult { text: line.to_string(), arcs: 0, segments: 0 })
    }

    fn update_pos(&mut self, words: &[(char, String)]) {
        let mut p = self.pos;
        for &(axis, ref value) in words {
            let Ok(n) = value.parse::<f64>() else { continue };
            let slot = match axis { 'X' => &mut p.x, 'Y' => &mut p.y, 'Z' => &mut p.z, _ => continue };
            *slot = if self.absolute { n } else { *slot + n };
        }
        self.pos = p;
    }

    fn convert_arc(&mut self, words: &[(char, String)], direction: Motion, plane: Plane) -> Result<Vec<String>, String> {
        let start = self.pos;
        let mut end = start;
        for &(axis, ref value) in words {
            let Ok(n) = value.parse::<f64>() else { continue };
            let slot = match axis { 'X' => &mut end.x, 'Y' => &mut end.y, 'Z' => &mut end.z, _ => continue };
            *slot = if self.absolute { n } else { *slot + n };
        }
        let (u0, v0, w0) = coords(start, plane);
        let (u1, v1, w1) = coords(end, plane);
        let (cu, cv, radius, start_angle, sweep) = if let Some(r) = word(words, 'R') {
            let r = r.parse::<f64>().map_err(|_| "invalid R radius")?;
            center_from_radius(u0, v0, u1, v1, r, direction)?
        } else {
            let (ou, ov) = match plane { Plane::XY => ('I','J'), Plane::XZ => ('K','I'), Plane::YZ => ('J','K') };
            let du = word(words, ou).unwrap_or("0").parse::<f64>().map_err(|_| "invalid arc-center offset")?;
            let dv = word(words, ov).unwrap_or("0").parse::<f64>().map_err(|_| "invalid arc-center offset")?;
            let center = if self.center_absolute { (du, dv) } else { (u0 + du, v0 + dv) };
            let radius = (u0 - center.0).hypot(v0 - center.1);
            if radius < EPS { return Err("arc radius is zero".into()); }
            let a0 = (v0 - center.1).atan2(u0 - center.0);
            let a1 = (v1 - center.1).atan2(u1 - center.0);
            let sweep = directed_sweep(a0, a1, direction, (u0-u1).hypot(v0-v1) < EPS);
            (center.0, center.1, radius, a0, sweep)
        };
        if !radius.is_finite() || radius < EPS { return Err("invalid arc radius".into()); }
        let chord = if MAX_SAGITTA >= radius { 2.0 * radius } else { 2.0 * (2.0 * radius * MAX_SAGITTA - MAX_SAGITTA * MAX_SAGITTA).sqrt() };
        let max_angle = if chord >= 2.0 * radius { MAX_STEP_ANGLE } else { MAX_STEP_ANGLE.min(2.0 * (chord / (2.0 * radius)).asin()) };
        let n = ((sweep.abs() / max_angle).ceil() as usize).max(1);
        let mut out = Vec::with_capacity(n);
        let mut previous = start;
        for i in 1..=n {
            let t = i as f64 / n as f64;
            let angle = start_angle + sweep * t;
            let u = if i == n { u1 } else { cu + radius * angle.cos() };
            let v = if i == n { v1 } else { cv + radius * angle.sin() };
            let w = w0 + (w1 - w0) * t;
            let p = from_coords(u, v, w, plane);
            let mut line = String::from("G1");
            let emitted = if self.absolute { p } else { Pos { x: p.x-previous.x, y: p.y-previous.y, z: p.z-previous.z } };
            let target = if self.absolute { end } else { Pos { x: end.x-previous.x, y: end.y-previous.y, z: end.z-previous.z } };
            append_axis(&mut line, 'X', emitted.x, target.x, i == n);
            append_axis(&mut line, 'Y', emitted.y, target.y, i == n);
            append_axis(&mut line, 'Z', emitted.z, target.z, i == n);
            if i == 1 { for &(letter, ref value) in words { if letter == 'F' { line.push(' '); line.push(letter); line.push_str(value); } } }
            out.push(line);
            previous = p;
        }
        self.pos = end;
        Ok(out)
    }
}

fn append_axis(line: &mut String, axis: char, value: f64, final_value: f64, last: bool) {
    // Emit the axes that define the arc plane and the interpolated third axis.
    let v = if last { final_value } else { value };
    line.push(' '); line.push(axis); line.push_str(&format!("{v:.5}"));
}
fn coords(p: Pos, plane: Plane) -> (f64,f64,f64) { match plane { Plane::XY => (p.x,p.y,p.z), Plane::XZ => (p.z,p.x,p.y), Plane::YZ => (p.y,p.z,p.x) } }
fn from_coords(u: f64,v: f64,w: f64,plane: Plane) -> Pos { match plane { Plane::XY => Pos{x:u,y:v,z:w}, Plane::XZ => Pos{x:v,y:w,z:u}, Plane::YZ => Pos{x:w,y:u,z:v} } }
fn directed_sweep(a0: f64, a1: f64, dir: Motion, full: bool) -> f64 {
    if full { return if matches!(dir, Motion::Cw) { -TAU } else { TAU }; }
    let ccw = (a1 - a0).rem_euclid(TAU);
    if matches!(dir, Motion::Ccw) { ccw } else { ccw - TAU }
}
fn center_from_radius(u0:f64,v0:f64,u1:f64,v1:f64,r_word:f64,dir:Motion)->Result<(f64,f64,f64,f64,f64),String>{
    let r=r_word.abs(); let du=u1-u0; let dv=v1-v0; let d=du.hypot(dv);
    if d<EPS || d>2.0*r+EPS { return Err("R arc has coincident endpoints or an impossible radius".into()); }
    let h=(r*r-(d/2.0).powi(2)).max(0.0).sqrt(); let mx=(u0+u1)/2.0; let my=(v0+v1)/2.0;
    for sign in [1.0,-1.0] { let cu=mx-sign*dv/d*h; let cv=my+sign*du/d*h; let a0=(v0-cv).atan2(u0-cu); let a1=(v1-cv).atan2(u1-cu); let sweep=directed_sweep(a0,a1,dir,false);
        if (r_word>0.0 && sweep.abs()<=std::f64::consts::PI+EPS)||(r_word<0.0 && sweep.abs()>std::f64::consts::PI-EPS) { return Ok((cu,cv,r,a0,sweep)); }
    }
    Err("could not resolve R arc center".into())
}
fn word<'a>(words:&'a [(char,String)], key:char)->Option<&'a str>{ words.iter().rev().find(|(c,_)|*c==key).map(|(_,v)|v.as_str()) }
fn split_comment(line:&str)->(&str,&str){ let semi=line.find(';').unwrap_or(line.len()); let paren=line.find('(').unwrap_or(line.len()); let i=semi.min(paren); (&line[..i],&line[i..]) }
fn format_lines(lines:Vec<String>,comment:&str)->String { let mut s=lines.join("\n"); if !comment.is_empty(){s.push(' ');s.push_str(comment);} s }
fn preserve_non_geometry_words(lines:&mut [String], words:&[(char,String)]) {
    let retained: Vec<String> = words.iter().filter_map(|(c,v)| {
        if matches!(*c,'N'|'M'|'S'|'P'|'Q') || (*c=='G' && v.parse::<f64>().map(|g| (g-2.0).abs() > EPS && (g-3.0).abs() > EPS).unwrap_or(true)) {
            Some(format!(" {c}{v}"))
        } else { None }
    }).collect();
    if let Some(first)=lines.first_mut() { for w in retained { first.push_str(&w); } }
}
fn parse_words(s:&str)->Result<Vec<(char,String)>,String>{
    let b=s.as_bytes(); let mut i=0; let mut out=Vec::new();
    while i<b.len(){ if b[i].is_ascii_whitespace(){i+=1;continue;} if !b[i].is_ascii_alphabetic(){return Err(format!("unexpected character '{}'",b[i] as char));}
        let letter=(b[i] as char).to_ascii_uppercase(); i+=1; let start=i;
        if i<b.len() && (b[i]==b'+'||b[i]==b'-'){i+=1;}
        let mut dot=false; while i<b.len(){if b[i].is_ascii_digit(){i+=1;}else if b[i]==b'.'&&!dot{dot=true;i+=1;}else{break;}}
        if start==i || (i==start+1 && (b[start]==b'+'||b[start]==b'-')) { return Err(format!("missing numeric value after {letter}")); }
        out.push((letter,s[start..i].to_string()));
    } Ok(out)
}
