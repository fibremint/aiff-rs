use super::extended::parse_extended_precision_bytes;
use super::{
    ids::{self, ChunkID},
    reader::{self, Buffer},
};
use id3;
use std::io::{Read, Seek, SeekFrom};
use std::ops::Div;

#[derive(Debug)]
pub enum ChunkError {
    InvalidID(ChunkID),
    InvalidFormType(ChunkID),
    InvalidID3Version([u8; 2]),
    InvalidSize(i32, i32),     // expected, got,
    InvalidData(&'static str), // failed to parse something
}

// TODO rename 'build'
pub trait Chunk<'a> {
    fn parse(
        buffer: Buffer<impl Read + Seek>,
        id: ChunkID,
        read_data: bool,
        curr_buf_pos: &mut Option<u64>
    ) -> Result<Option<Self>, ChunkError>
    where
        Self: Sized + 'a;
}

// TODO different form chunks based on parsing options? lighter weight
// can a macro help make this dynamic / implement every possible version?
// CompletedFormChunk, with only required props
// CompletedFormChunkWithMeta, with all metadata
#[derive(Debug)]
pub struct FormChunk {
    // size: i32,                     // required
    common: Option<CommonChunk>,   // required
    sound: Option<SoundDataChunk>, // required if num_sample_frames > 0
    comments: Option<CommentsChunk>,
    instrument: Option<InstrumentChunk>,
    recording: Option<AudioRecordingChunk>,
    texts: Option<Vec<TextChunk>>,
    markers: Option<Vec<MarkerChunk>>,
    midi: Option<Vec<MIDIDataChunk>>,
    apps: Option<Vec<ApplicationSpecificChunk>>,
}

impl FormChunk {
    pub fn common(&self) -> &Option<CommonChunk> {
        &self.common
    }

    pub fn set_common(&mut self, c: CommonChunk) {
        self.common = Some(c);
    }

    pub fn sound(&self) -> &Option<SoundDataChunk> {
        &self.sound
    }

    pub fn set_sound(&mut self, c: SoundDataChunk) {
        self.sound = Some(c);
    }

    pub fn set_comments(&mut self, c: CommentsChunk) {
        self.comments = Some(c)
    }

    pub fn set_instrument(&mut self, c: InstrumentChunk) {
        self.instrument = Some(c)
    }

    pub fn set_recording(&mut self, c: AudioRecordingChunk) {
        self.recording = Some(c)
    }

    pub fn add_text_chunk(&mut self, c: TextChunk) {
        if self.texts.is_none() {
            self.texts = Some(vec![]);
        }
        if let Some(t) = &mut self.texts {
            t.push(c);
        } else {
            panic!("vec should exist at this point")
        }
    }

    pub fn add_marker_chunk(&mut self, c: MarkerChunk) {
        if self.markers.is_none() {
            self.markers = Some(vec![]);
        }
        if let Some(m) = &mut self.markers {
            m.push(c);
        } else {
            panic!("vec should exist at this point")
        }
    }

    pub fn add_midi_chunk(&mut self, c: MIDIDataChunk) {
        if self.midi.is_none() {
            self.midi = Some(vec![]);
        }
        if let Some(m) = &mut self.midi {
            m.push(c);
        } else {
            panic!("vec should exist at this point")
        }
    }

    pub fn add_app_chunk(&mut self, c: ApplicationSpecificChunk) {
        if self.apps.is_none() {
            self.apps = Some(vec![]);
        }
        if let Some(a) = &mut self.apps {
            a.push(c);
        } else {
            panic!("vec should exist at this point")
        }
    }

    pub fn duration(&self) -> Option<f64> {
        if let Some(common) = &self.common {
            Some((common.num_sample_frames as f64).div(common.sample_rate))
        } else {
            None
        }
    }
}

impl Chunk<'_> for FormChunk {
    fn parse(
        buf: Buffer<impl Read + Seek>,
        id: ChunkID,
        read_data: bool,
        curr_buf_pos: &mut Option<u64>
    ) -> Result<Option<FormChunk>, ChunkError> {
        if let Some(ref mut pos) = curr_buf_pos {
            *pos = buf.position();
        }

        if &id != ids::FORM {
            return Err(ChunkError::InvalidID(id));
        }

        let size = reader::read_i32_be(buf);
        println!("form chunk bytes {}", size);

        if !read_data {
            buf.seek(SeekFrom::Current(4)).unwrap();

            return Ok(None);
        }

        let mut form_type = [0; 4];
        buf.read_exact(&mut form_type).unwrap();

        match &form_type {
            ids::AIFF => Ok(Some(
                FormChunk {
                    // size,
                    common: None,
                    sound: None,
                    comments: None,
                    instrument: None,
                    recording: None,
                    texts: None,
                    markers: None,
                    midi: None,
                    apps: None,
                }
            )),
            ids::AIFF_C => {
                println!("aiff c file detected; unsupported");
                Err(ChunkError::InvalidFormType(form_type))
            }
            &x => Err(ChunkError::InvalidFormType(x)),
        }
    }
}

#[derive(Debug)]
pub struct CommonChunk {
    pub size: i32,
    pub num_channels: i16,
    pub num_sample_frames: u32,
    pub bit_rate: i16, // in the spec, this is defined as `sample_size`
    pub sample_rate: f64, // 80 bit extended floating pt num
}

impl Chunk<'_> for CommonChunk {
    fn parse(
        buf: Buffer<impl Read + Seek>,
        id: ChunkID,
        read_data: bool,
        curr_buf_pos: &mut Option<u64>
    ) -> Result<Option<CommonChunk>, ChunkError> {
        if let Some(ref mut pos) = curr_buf_pos {
            *pos = buf.position();
        }

        if &id != ids::COMMON {
            return Err(ChunkError::InvalidID(id));
        }

        let (size, num_channels, num_sample_frames, bit_rate) = (
            reader::read_i32_be(buf),
            reader::read_i16_be(buf),
            reader::read_u32_be(buf),
            reader::read_i16_be(buf),
        );

        if !read_data {
            buf.seek(SeekFrom::Current(10)).unwrap();

            return Ok(None)
        }
        
        let mut rate_buf = [0; 10]; // 1 bit sign, 15 bits exponent
        buf.read_exact(&mut rate_buf).unwrap();

        let sample_rate = match parse_extended_precision_bytes(rate_buf) {
            Ok(s) => s,
            Err(()) => {
                return Err(ChunkError::InvalidData("Extended Precision"))
            }
        };

        Ok(Some(
            CommonChunk {
                size,
                num_channels,
                num_sample_frames,
                bit_rate,
                sample_rate,
            }
        ))
    }
}

#[derive(Debug)]
pub struct SoundDataChunk {
    pub size: i32,
    pub offset: u32,
    pub block_size: u32,
    pub sound_data: Vec<u8>,
}

impl Chunk<'_> for SoundDataChunk {
    fn parse(
        buf: Buffer<impl Read + Seek>,
        id: ChunkID,
        read_data: bool,
        curr_buf_pos: &mut Option<u64>
    ) -> Result<Option<SoundDataChunk>, ChunkError> {
        if let Some(ref mut pos) = curr_buf_pos {
            *pos = buf.position();
        }

        if &id != ids::SOUND {
            return Err(ChunkError::InvalidID(id));
        }

        let size = reader::read_i32_be(buf);
        let offset = reader::read_u32_be(buf);
        let block_size = reader::read_u32_be(buf);

        if !read_data {
            buf.seek(SeekFrom::Current(size as i64)).unwrap();

            return Ok(None);
        }

        // TODO some sort of streaming read optimization?
        // let sound_size = size - 8; // account for offset + block size bytes
        let mut sound_data = vec![0u8; size as usize];
        // let mut sound_data = vec![0u8; sound_size as usize];

        buf.read_exact(&mut sound_data).unwrap();

        Ok(Some(
            SoundDataChunk {
                size,
                offset,
                block_size,
                sound_data,
            }
        ))
    }
}

type MarkerId = i16;
#[derive(Debug)]
pub struct Marker {
    id: MarkerId,
    position: u32,
    marker_name: String,
}

impl Marker {
    // TODO return result
    pub fn from_reader<R: Read + Seek>(r: &mut R) -> Marker {
        let id = reader::read_i16_be(r);
        let position = reader::read_u32_be(r);
        let marker_name = reader::read_pstring(r);

        Marker {
            id,
            position,
            marker_name,
        }
    }
}

#[derive(Debug)]
pub struct MarkerChunk {
    pub size: i32,
    pub num_markers: u16,
    pub markers: Vec<Marker>,
}

impl Chunk<'_> for MarkerChunk {
    fn parse(
        buf: Buffer<impl Read + Seek>,
        id: ChunkID,
        read_data: bool,
        curr_buf_pos: &mut Option<u64>
    ) -> Result<Option<MarkerChunk>, ChunkError> {
        if let Some(ref mut pos) = curr_buf_pos {
            *pos = buf.position();
        }

        if &id != ids::MARKER {
            return Err(ChunkError::InvalidID(id));
        }

        let size = reader::read_i32_be(buf);
        let num_markers = reader::read_u16_be(buf);

        // if !read_data {
        //     buf.seek(pos)
        // }
        let mut markers = Vec::with_capacity(num_markers as usize);
        // is it worth it to read all markers at once ant create from buf?
        // or does the usage of BufReader make it irrelevant?
        for _ in 0..num_markers {
            markers.push(Marker::from_reader(buf));
        }

        Ok(Some(
            MarkerChunk {
                size,
                num_markers,
                markers,
            }
        ))
    }
}

#[derive(Debug)]
pub enum TextChunkType {
    Name,
    Author,
    Copyright,
    Annotation,
}

#[derive(Debug)]
pub struct TextChunk {
    pub chunk_type: TextChunkType,
    pub size: i32,
    pub text: String,
}

impl Chunk<'_> for TextChunk {
    fn parse(
        buf: Buffer<impl Read + Seek>,
        id: ChunkID,
        read_data: bool,
        curr_buf_pos: &mut Option<u64>
    ) -> Result<Option<TextChunk>, ChunkError> {
        if let Some(ref mut pos) = curr_buf_pos {
            *pos = buf.position();
        }

        let chunk_type = match &id {
            ids::NAME => TextChunkType::Name,
            ids::AUTHOR => TextChunkType::Author,
            ids::COPYRIGHT => TextChunkType::Copyright,
            ids::ANNOTATION => TextChunkType::Annotation,
            _ => return Err(ChunkError::InvalidID(id)),
        };

        let size = reader::read_i32_be(buf);
        let buf_pos_offset = if size % 2 > 0 { 1 } else { 0 };

        if !read_data {
            buf.seek(SeekFrom::Current(size as i64 + buf_pos_offset)).unwrap();

            return Ok(None);
        }

        let mut text_bytes = vec![0; size as usize];
        buf.read_exact(&mut text_bytes).unwrap();
        let text = String::from_utf8(text_bytes).unwrap();

        buf.seek(SeekFrom::Current(buf_pos_offset)).unwrap();
        // if size % 2 > 0 {
        //     // if odd, pad byte present - skip it
        //     buf.seek(SeekFrom::Current(1)).unwrap();
        // }

        Ok(Some(
            TextChunk {
                chunk_type,
                size,
                text,
            }
        ))
    }
}

#[derive(Debug)]
pub struct Loop {
    // 0 no looping / 1 foward loop / 2 forward backward loop - use enum?
    play_mode: i16,
    begin_loop: MarkerId,
    end_loop: MarkerId,
}

impl Loop {
    // TODO return result
    pub fn from_reader(r: &mut impl Read) -> Loop {
        let play_mode = reader::read_i16_be(r);
        let begin_loop = reader::read_i16_be(r);
        let end_loop = reader::read_i16_be(r);

        Loop {
            play_mode,
            begin_loop,
            end_loop,
        }
    }
}

// midi note value range = 0..127 (? not the full range?)
#[derive(Debug)]
pub struct InstrumentChunk {
    size: i32,
    base_note: i8,     // MIDI
    detune: i8,        // -50..50
    low_note: i8,      // MIDI
    high_note: i8,     // MIDI
    low_velocity: i8,  // MIDI
    high_velocity: i8, // MIDI
    gain: i16,         // in db
    sustain_loop: Loop,
    release_loop: Loop,
}

impl Chunk<'_> for InstrumentChunk {
    fn parse(
        buf: Buffer<impl Read + Seek>,
        id: ChunkID,
        read_data: bool,
        curr_buf_pos: &mut Option<u64>
    ) -> Result<Option<InstrumentChunk>, ChunkError> {
        if let Some(ref mut pos) = curr_buf_pos {
            *pos = buf.position();
        }

        if &id != ids::INSTRUMENT {
            return Err(ChunkError::InvalidID(id));
        }

        let size = reader::read_i32_be(buf);
        let base_note = reader::read_i8_be(buf);
        let detune = reader::read_i8_be(buf);
        let low_note = reader::read_i8_be(buf);
        let high_note = reader::read_i8_be(buf);
        let low_velocity = reader::read_i8_be(buf);
        let high_velocity = reader::read_i8_be(buf);
        let gain = reader::read_i16_be(buf);

        let sustain_loop = Loop::from_reader(buf);
        let release_loop = Loop::from_reader(buf);

        Ok(Some(
            InstrumentChunk {
                size,
                base_note,
                detune,
                low_note,
                high_note,
                low_velocity,
                high_velocity,
                gain,
                sustain_loop,
                release_loop,
            }
        ))
    }
}

#[derive(Debug)]
pub struct MIDIDataChunk {
    size: i32,
    data: Vec<u8>,
}

impl Chunk<'_> for MIDIDataChunk {
    fn parse(
        buf: Buffer<impl Read + Seek>,
        id: ChunkID,
        read_data: bool,
        curr_buf_pos: &mut Option<u64>
    ) -> Result<Option<MIDIDataChunk>, ChunkError> {
        if let Some(ref mut pos) = curr_buf_pos {
            *pos = buf.position();
        }

        if &id != ids::MIDI {
            return Err(ChunkError::InvalidID(id));
        }

        let size = reader::read_i32_be(buf);

        if !read_data {
            buf.seek(SeekFrom::Current(size as i64)).unwrap();

            return Ok(None);
        }

        let mut data = vec![0; size as usize];
        buf.read_exact(&mut data).unwrap();

        Ok(Some(
            MIDIDataChunk { size, data }
        ))
    }
}

#[derive(Debug)]
pub struct AudioRecordingChunk {
    size: i32,
    // AESChannelStatusData
    // specified in "AES Recommended Practice for Digital Audio Engineering"
    data: [u8; 24],
}

impl Chunk<'_> for AudioRecordingChunk {
    fn parse(
        buf: Buffer<impl Read + Seek>,
        id: ChunkID,
        read_data: bool,
        curr_buf_pos: &mut Option<u64>
    ) -> Result<Option<AudioRecordingChunk>, ChunkError> {
        if let Some(ref mut pos) = curr_buf_pos {
            *pos = buf.position();
        }

        if &id != ids::RECORDING {
            return Err(ChunkError::InvalidID(id));
        }

        let size = reader::read_i32_be(buf);
        if size != 24 {
            return Err(ChunkError::InvalidSize(24, size));
        }

        if !read_data {
            buf.seek(SeekFrom::Current(24)).unwrap();

            return Ok(None);
        }

        let mut data = [0; 24];
        buf.read_exact(&mut data).unwrap();

        Ok(Some(AudioRecordingChunk { size, data }))
    }
}

#[derive(Debug)]
pub struct ApplicationSpecificChunk {
    size: i32,
    application_signature: ChunkID, // TODO check if bytes should be i8
    data: Vec<i8>,
}

impl Chunk<'_> for ApplicationSpecificChunk {
    fn parse(
        buf: Buffer<impl Read + Seek>,
        id: ChunkID,
        read_data: bool,
        curr_buf_pos: &mut Option<u64>
    ) -> Result<Option<ApplicationSpecificChunk>, ChunkError> {
        if let Some(ref mut pos) = curr_buf_pos {
            *pos = buf.position();
        }

        if &id != ids::APPLICATION {
            return Err(ChunkError::InvalidID(id));
        }

        let size = reader::read_i32_be(buf);
        let application_signature = reader::read_chunk_id(buf); // TODO verify
        
        if !read_data {
            buf.seek(SeekFrom::Current((size - 4) as i64)).unwrap();

            return Ok(None);
        }
        
        let mut data = vec![0; (size - 4) as usize]; // account for sig size
        buf.read_exact(&mut data).unwrap();

        Ok(Some(
            ApplicationSpecificChunk {
                size,
                application_signature,
                data: data.iter().map(|byte| i8::from_be_bytes([*byte])).collect(),
            }
        ))
    }
}

#[derive(Debug)]
pub struct Comment {
    timestamp: u32,
    marker_id: MarkerId,
    count: u16,
    text: String, // padded to an even # of bytes
}

impl Comment {
    // TODO return result
    pub fn from_reader(r: &mut impl Read) -> Comment {
        let timestamp = reader::read_u32_be(r);
        let marker_id = reader::read_i16_be(r);
        let count = reader::read_u16_be(r);

        let mut str_buf = vec![0; count as usize];
        r.read_exact(&mut str_buf).unwrap();
        let text = String::from_utf8(str_buf).unwrap();

        Comment {
            timestamp,
            marker_id,
            count,
            text,
        }
    }
}

#[derive(Debug)]
pub struct CommentsChunk {
    size: i32,
    num_comments: u16,
    comments: Vec<Comment>,
}

impl Chunk<'_> for CommentsChunk {
    fn parse(
        buf: Buffer<impl Read + Seek>,
        id: ChunkID,
        read_data: bool,
        curr_buf_pos: &mut Option<u64>
    ) -> Result<Option<CommentsChunk>, ChunkError> {
        if let Some(ref mut pos) = curr_buf_pos {
            *pos = buf.position();
        }

        if &id != ids::COMMENTS {
            return Err(ChunkError::InvalidID(id));
        }

        let size = reader::read_i32_be(buf);
        let num_comments = reader::read_u16_be(buf);

        let mut comments = Vec::with_capacity(num_comments as usize);
        for _ in 0..num_comments {
            comments.push(Comment::from_reader(buf))
        }

        Ok(Some(
            CommentsChunk {
                size,
                num_comments,
                comments,
            }
        ))
    }
}

// #[derive(Debug)]
// pub struct ID3v1Chunk {}

// impl Chunk for ID3v1Chunk {
//     fn parse(
//         buf: Buffer<impl Read + Seek>,
//         id: ChunkID,
//     ) -> Result<ID3v1Chunk, ChunkError> {
//     }
// }

// TODO store id3 franes
#[derive(Debug)]
pub struct ID3v2Chunk {
    // // version: [u8; 2],
    // pub artist: Option<String>,
    // pub album: Option<String>,
    // pub album_artist: Option<String>,
    // pub date_recorded: Option<id3::Timestamp>,
    // pub date_released: Option<id3::Timestamp>,
    // pub disc: Option<u32>,
    // pub duration: Option<u32>,
    // pub genre: Option<String>,
    // // // pictures: Option<&'a id3::frame::Picture>,
    // // pictures: Vec<id3::frame::Picture>,
    // // title: Option<&'a str>,
    // // total_discs: Option<u32>,
    // // total_tracks: Option<u32>,
    // // track: Option<u32>,
    // // year: Option<i32>,
    pub tag: id3::Tag,
}

// should this be an optional feature? maybe consumer already has id3 parsing
impl Chunk<'_> for ID3v2Chunk {
    fn parse(
        buf: Buffer<impl Read + Seek>,
        id: ChunkID,
        read_data: bool,
        curr_buf_pos: &mut Option<u64>
    ) -> Result<Option<ID3v2Chunk>, ChunkError> {
        if let Some(ref mut pos) = curr_buf_pos {
            *pos = buf.position();
        }

        if &id[0..3] != ids::ID3 && &id[1..] != ids::ID3 {
            return Err(ChunkError::InvalidID(id));
        }

        // TODO is this necessary? can we get this from id3 read
        let mut version = [0; 2];
        buf.seek(SeekFrom::Current(3)).unwrap();
        buf.read_exact(&mut version).unwrap();
        buf.seek(SeekFrom::Current(-5)).unwrap();

        // major versions up to 2.4, no minor versions known
        if version[0] > 4 || version[1] != 0 {
            return Err(ChunkError::InvalidID3Version(version));
        }

        // buffer MUST start with "ID3" or this call will fail
        let tag = id3::Tag::read_from(buf).unwrap();
        // // let mut _artist = "";
        // // let artist = tag.artist().unwrap().to_owned();
        // // let artist = Some(tag.artist().unwrap_or_default().to_owned());
        // let artist = match tag.artist() {
        //     Some(item) => Some(item.to_owned()),
        //     None => None
        // };
        // // let album = tag.album().to_owned();
        // // let album = Some(tag.album().unwrap_or_default().to_owned());
        // let album = match tag.album() {
        //     Some(item) => Some(item.to_owned()),
        //     None => None,
        // };
        // // let album_artist = tag.album_artist().to_owned();
        // // let album_artist = Some(tag.album_artist().unwrap_or_default().to_owned());
        // let album_artist = match tag.album_artist() {
        //     Some(item) => Some(item.to_owned()),
        //     None => None,
        // };
        // // let comments = tag.comments();
        // let date_recorded = tag.date_recorded().to_owned();
        // // let date_recorded = Some(tag.date_recorded().to_owned());
        // let date_released = tag.date_released().to_owned();
        // let disc = tag.disc().to_owned();
        // let duration = tag.duration().to_owned();
        // // let extended_links = tag.extended_links();
        // // let extended_texts = tag.extended_texts();
        // // let genre = tag.genre().to_owned();
        // let genre = match tag.genre() {
        //     Some(item) => Some(item.to_owned()),
        //     None => None,
        // };
        // // let lyrics = tag.lyrics();
        // let pictures = tag.pictures();
        // let title = tag.title().to_owned();
        // let total_discs = tag.total_discs().to_owned();
        // let total_tracks = tag.total_tracks().to_owned();
        // let track = tag.track().to_owned();
        // let year = tag.year().to_owned();

        // println!("artist: {:?}, album: {:?}, album_artist: {:?}, date_recorded: {:?}, date_released: {:?}, disc: {:?}, duration: {:?}, genre: {:?}, title: {:?}, total_discs: {:?}
        //           total_tracks: {:?}, track: {:?}, year: {:?}", 
        //          artist, album, album_artist, date_recorded, date_released, disc, duration, genre,
        //          title, total_discs, total_tracks, track, year);

        // let picture: Vec<_> = pictures.collect();
        // let picture: Vec<_> = pictures.into_iter().map(|item| item.to_owned()).collect();
        // println!("picture: {:?}", picture);
        
        // let frames: Vec<_> = tag.frames().collect();
        // println!("id3 frames {:?}", frames);

        // Ok(ID3v2Chunk { version })

        // Ok(ID3v2Chunk {
        //     artist,
        //     album,
        //     album_artist,
        //     date_recorded,
        //     date_released,
        //     disc,
        //     duration,
        //     genre,
        //     // pictures: picture,
        //     // title,
        //     // total_discs,
        //     // total_tracks,
        //     // track,
        //     // year,
        // })
        Ok(Some(
            ID3v2Chunk {
                tag,
            }
        ))
    }
}
