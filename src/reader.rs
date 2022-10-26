use super::{
    chunks::{self, Chunk, FormChunk},
    ids,
    samples::SampleType,
};
use seek_bufread::BufReader;
use std::{io::{Read, Seek, SeekFrom}, hash::Hash, convert::TryInto};
use std::collections::HashMap;

pub type Buffer<'a, Source> = &'a mut BufReader<Source>;

// TODO samples iterator, enable seeking by duration fn
// TODO diffeerent types of reader structs?
// AiffAudioReader / AiffCompleteReader (id3 optional)
pub struct AiffReader<Source> {
    buf: BufReader<Source>,
    pub form_chunk: Option<FormChunk>,
    // pub id3v1_tags: Vec<chunks::ID3v1Chunk>, // should this be optional? or separate
    // pub id3v2_tags: Vec<chunks::ID3v2Chunk>, // should this be optional? or separate
    pub id3v2_tag: Option<id3::Tag>,
    form_buf_locations: HashMap<String, u64>,
}

impl<Source: Read + Seek> AiffReader<Source> {
    pub fn new(s: Source) -> AiffReader<Source> {
        AiffReader {
            buf: BufReader::new(s),
            form_chunk: None,
            id3v2_tag: None,
            form_buf_locations: HashMap::new(),
            // id3v2_tags: vec![],
            // id3v1_tags: vec![],
        }
    }

    pub fn read_all_form_data(&mut self) {
        self.analyze_data(true, false).unwrap();
    }

    pub fn parse_form_location(&mut self) -> Result<(), chunks::ChunkError> {
        self.analyze_data(false, true).unwrap();

        Ok(())
    }

    pub fn read_chunk<'a, T: Chunk<'a> + 'a> (&mut self, read_data: bool, record_form_pos: bool, chunk_id: &[u8]) -> Option<T> {
        let tag_id = String::from_utf8(chunk_id.to_vec()).unwrap();
        let mut form_pos = if record_form_pos { Some(0) } else { None };

        if let Some(seek_pos) = self.form_buf_locations.get(&tag_id) {
            self.buf.seek(SeekFrom::Start(*seek_pos)).unwrap();
        }

        let chunk = T::parse(&mut self.buf, chunk_id.try_into().unwrap(), read_data, &mut form_pos).unwrap();

        if let Some(pos) = form_pos {
            self.form_buf_locations.insert(tag_id, pos);
        }

        chunk
    }

    fn analyze_data(&mut self, read_data: bool, record_form_pos: bool) -> Result<(), chunks::ChunkError> {
        self.buf.rewind().unwrap();

        let form_id = read_chunk_id(&mut self.buf);
        let mut form = match self.read_chunk::<chunks::FormChunk>(true, record_form_pos, &form_id) {
            Some(item) => item,
            None => return Err(chunks::ChunkError::InvalidData("failed to parse form data"))
        };

        while self.buf.available() >= 4 {
            let id = read_chunk_id(&mut self.buf);

            // once the common and form are detected, we can loop
            // buffer position is right past the id
            match &id {
                ids::COMMON => {
                    // println!("Common chunk detected");
                    if let Some(common) = self.read_chunk::<chunks::CommonChunk>(read_data, record_form_pos, &id) {
                        form.set_common(common);
                    }
                }
                ids::SOUND => {
                    if let Some(sound) = self.read_chunk::<chunks::SoundDataChunk>(read_data, record_form_pos, &id) {
                        form.set_sound(sound);
                    }
                }
                ids::MARKER => {
                    if let Some(mark) = self.read_chunk::<chunks::MarkerChunk>(read_data, record_form_pos, &id) {
                        form.add_marker_chunk(mark);
                    }
                }
                ids::INSTRUMENT => {
                    if let Some(inst) = self.read_chunk::<chunks::InstrumentChunk>(read_data, record_form_pos, &id) {
                        form.set_instrument(inst);
                    }
                }
                ids::MIDI => {
                    if let Some(midi) = self.read_chunk::<chunks::MIDIDataChunk>(read_data, record_form_pos, &id) {
                        form.add_midi_chunk(midi);
                    }
                }
                ids::RECORDING => {
                    if let Some(midi) = self.read_chunk::<chunks::AudioRecordingChunk>(read_data, record_form_pos, &id) {
                        form.set_recording(midi);
                    }
                }
                ids::APPLICATION => {
                    if let Some(app) = self.read_chunk::<chunks::ApplicationSpecificChunk>(read_data, record_form_pos, &id) {
                        form.add_app_chunk(app);
                    }
                }
                ids::COMMENTS => {
                    if let Some(comm) = self.read_chunk::<chunks::CommentsChunk>(read_data, record_form_pos, &id) {
                        form.set_comments(comm);
                    }
                }
                ids::NAME | ids::AUTHOR | ids::COPYRIGHT | ids::ANNOTATION => {
                    if let Some(text) = self.read_chunk::<chunks::TextChunk>(read_data, record_form_pos, &id) {
                        form.add_text_chunk(text);
                    }
                }
                ids::FVER => {
                    unimplemented!("FVER chunk detected");
                }
                // 3 bytes "ID3" identifier
                // TODO merge both options
                // ID3 chunks aren't stored in the FORM chunk. should they
                // be stored next to the form chunk in the reader?
                [73, 68, 51, _] => {
                    self.buf.seek(SeekFrom::Current(-4)).unwrap();

                    match self.read_chunk::<chunks::ID3v2Chunk>(read_data, record_form_pos, &id) {
                        // Ok(chunk) => self.id3v2_tags.push(chunk),
                        Some(chunk) => self.id3v2_tag = Some(chunk.tag),
                        None => {
                            println!("Build ID3 chunk failed");
                            self.buf.seek(SeekFrom::Current(3)).unwrap();
                        },
                        _ => ()
                    }
                }
                [_, 73, 68, 51] => {
                    self.buf.seek(SeekFrom::Current(-3)).unwrap();

                    match self.read_chunk::<chunks::ID3v2Chunk>(read_data, record_form_pos, ids::ID3) {
                        // Ok(chunk) => self.id3v2_tags.push(chunk),
                        Some(chunk) => self.id3v2_tag = Some(chunk.tag),
                        None => {
                            println!("Build ID3 chunk failed");
                            self.buf.seek(SeekFrom::Current(3)).unwrap();
                        },
                        _ => ()
                    }

                }
                [84, 65, 71, _] => println!("v1 id3"), // "TAG_"
                [_, 84, 65, 71] => println!("v1 id3"), // "_TAG"
                ids::CHAN | ids::BASC | ids::TRNS | ids::CATE => {
                    unimplemented!("apple stuff detected")
                }
                id => println!(
                    "other chunk {:?} {:?}",
                    id,
                    String::from_utf8_lossy(id)
                ),
                // _ => (),
            };
        }
        self.form_chunk = Some(form);

        // FIXME handle remaining bytes
        println!("buffer complete {} byte(s) left", self.buf.available());
        // set position to end?

        Ok(())
    }

    pub fn form(&self) -> &Option<FormChunk> {
        &self.form_chunk
    }

    // TODO need to check available
    // TODO return result iterator or complete buffer of data
    // TODO pack frams
    // should return a generic AiffSample<u8/u16/u32> etc
    // TODO samples is most likely integers

    pub fn samples<T: SampleType>(&self) -> Vec<T> {
        let f = self.form_chunk.as_ref().unwrap();
        let s = f.sound().as_ref().unwrap();
        let c = f.common().as_ref().unwrap();

        // a sample point is the sound data for a single channel of audio
        // sample points containn <bit_rate> bits of data
        // a sample frame contains sample points for all channels
        // playback occurs at <sample_rate> frames per second
        // num samples is always > 0 so shouldn't be any conversion issues
        // maybe it should be stored as a u16?
        let sample_points =
            (c.num_sample_frames * c.num_channels as u32) as usize;
        println!("sample points {:?}", sample_points);

        let mut samples = Vec::with_capacity(sample_points);
        let mut bytes_per_point = (c.bit_rate / 8) as usize;
        if c.bit_rate % 8 != 0 {
            bytes_per_point += 1;
        }

        for point in 0..sample_points {
            samples.push(T::parse(&s.sound_data, point * bytes_per_point, c.bit_rate));
        }

        samples
    }

    // TODO create samples iterator for better performance
}

// enums are always the max possible size, so neeeds to be structs and traits

// TODO remove panics
// TODO move these into their own file - what's a good name?

pub fn read_chunk_id(r: &mut impl Read) -> ids::ChunkID {
    let mut id = [0; 4];
    if let Err(e) = r.read_exact(&mut id) {
        panic!("unable to read_u8 {:?}", e)
    }
    id
}

pub fn read_u8(r: &mut impl Read) -> u8 {
    let mut b = [0; 1];
    if let Err(e) = r.read_exact(&mut b) {
        panic!("unable to read_u8 {:?}", e)
    }
    b[0]
}

pub fn read_u16_be(r: &mut impl Read) -> u16 {
    let mut b = [0; 2];
    if let Err(e) = r.read_exact(&mut b) {
        panic!("unable to read_u8 {:?}", e)
    }
    u16::from_be_bytes(b)
}

pub fn read_u32_be(r: &mut impl Read) -> u32 {
    let mut b = [0; 4];
    if let Err(e) = r.read_exact(&mut b) {
        panic!("unable to read_i32_be {:?}", e)
    }
    u32::from_be_bytes(b)
}

pub fn read_i8_be(r: &mut impl Read) -> i8 {
    let mut b = [0; 1];
    if let Err(e) = r.read_exact(&mut b) {
        panic!("unable to read_i32_be {:?}", e)
    }
    i8::from_be_bytes(b)
}

pub fn read_i16_be(r: &mut impl Read) -> i16 {
    let mut b = [0; 2];
    if let Err(e) = r.read_exact(&mut b) {
        panic!("unable to read_i32_be {:?}", e)
    }
    i16::from_be_bytes(b)
}

pub fn read_i32_be(r: &mut impl Read) -> i32 {
    let mut b = [0; 4];
    if let Err(e) = r.read_exact(&mut b) {
        panic!("unable to read_i32_be {:?}", e)
    }
    i32::from_be_bytes(b)
}

// TODO testme with pascal strings
pub fn read_pstring<R: Read + Seek>(r: &mut R) -> String {
    let len = read_u8(r);
    let mut str_buf = vec![0; len as usize];
    r.read_exact(&mut str_buf).unwrap();

    if len % 2 > 0 {
        // skip pad byte if odd
        r.seek(SeekFrom::Current(1)).unwrap();
    }

    String::from_utf8(str_buf).unwrap()
}
