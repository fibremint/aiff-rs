use super::ids;
use bytes::buf::Buf;
use rust_decimal::Decimal;
use std::io::Cursor;

type Buffer<'a> = &'a mut Cursor<Vec<u8>>;

// The first 8 bytes of a chunk are chunk ID and chunk size
pub struct ChunkBuilder(ids::ChunkID);

// The 'parse' fns could return a dynamic type
// but this allows us to lean on type checking, right?
// otherwise we still have to figure out what type is returned anyway
impl ChunkBuilder {
    pub fn new(buffer: Buffer) -> ChunkBuilder {
        // TODO check size
        let mut id = [0; 4];
        buffer.copy_to_slice(&mut id);
        ChunkBuilder(id)
    }

    pub fn id(&self) -> &ids::ChunkID {
        let ChunkBuilder(id) = &self;
        id
    }

    pub fn consume(self) -> ids::ChunkID {
        let ChunkBuilder(id) = self;
        id
    }
}

#[derive(Debug)]
pub enum ChunkError {
    InvalidID(ids::ChunkID),
    InvalidFormType(ids::ChunkID),
    InvalidID3Version([u8; 2]),
}

pub trait Chunk {
    fn build(cb: ChunkBuilder, buffer: Buffer) -> Result<Self, ChunkError>
    where
        Self: Sized;
}

// container for all other chunks in file
// TODO return CompleteFormChunk when CommonChunk + SoundChunk is present?
// assuming we can't rely on the common chunk being at the start
pub struct FormChunk {
    size: i32,
    common: Option<CommonChunk>,
    sound: Option<SoundDataChunk>,
    chunks: Vec<Box<dyn Chunk>>,
}

impl FormChunk {
    fn add_common(&mut self, chunk: CommonChunk) { self.common = Some(chunk); }

    fn add_sound(&mut self, chunk: SoundDataChunk) { self.sound = Some(chunk); }

    fn add_chunk(&mut self, chunk: Box<dyn Chunk>) { self.chunks.push(chunk); }

    // TODO move to ChunkReader
    // Reader should create chunk builders and pass them to a fn
    // here called 'add_chunk'.
    pub fn load_chunks(&mut self, buf: Buffer) -> Result<(), ChunkError> {
        while buf.remaining() >= 4 {
            let cb = ChunkBuilder::new(buf);

            // once the common and form are detected, we can loop
            match cb.id() {
                ids::COMMON => {
                    println!("Common chunk detected");
                    let common = CommonChunk::build(cb, buf).unwrap();
                    println!(
                        "channels {} frames {} size {} rate {}",
                        common.num_channels,
                        common.num_sample_frames,
                        common.sample_size,
                        common.sample_rate
                    );
                    self.add_common(common);
                }
                ids::SOUND => {
                    println!("SOUND chunk detected");
                    let sound = SoundDataChunk::build(cb, buf).unwrap();
                    println!(
                        "size {} offset {} block size {}",
                        sound.size, sound.offset, sound.block_size
                    );
                    self.add_sound(sound);
                }
                ids::MARKER => println!("MARKER chunk detected"),
                ids::INSTRUMENT => println!("INSTRUMENT chunk detected"),
                ids::MIDI => println!("MIDI chunk detected"),
                ids::RECORDING => println!("RECORDING chunk detected"),
                ids::APPLICATION => println!("APPLICATION chunk detected"),
                ids::COMMENT => println!("COMMENT chunk detected"),
                ids::NAME | ids::AUTHOR | ids::COPYRIGHT | ids::ANNOTATION => {
                    let text = TextChunk::build(cb, buf).unwrap();
                    println!("TEXT chunk detected: {}", text.text);
                    self.add_chunk(Box::new(text));
                }
                ids::FVER => {
                    println!("FVER chunk detected");
                    unimplemented!();
                }
                // 3 bytes "ID3" identifier. 4th byte is first version byte
                [73, 68, 51, _x] => match ID3Chunk::build(cb, buf) {
                    Ok(chunk) => self.add_chunk(Box::new(chunk)),
                    Err(e) => println!("Build ID3 chunk failed {:?}", e),
                },
                ids::CHAN | ids::BASC | ids::TRNS | ids::CATE => {
                    println!("apple stuff detected")
                }
                _ => (),
                //                id => println!("other chunk {:?}", id),
            }
        }

        // FIXME handle remaining bytes
        println!("buffer complete {}", buf.remaining());

        Ok(())
    }
}

impl Chunk for FormChunk {
    fn build(cb: ChunkBuilder, buf: Buffer) -> Result<FormChunk, ChunkError> {
        if cb.id() != ids::FORM {
            return Err(ChunkError::InvalidID(cb.consume()));
        }

        let size = buf.get_i32_be();
        let mut form_type = [0; 4];
        buf.copy_to_slice(&mut form_type);

        match &form_type {
            ids::AIFF => Ok(FormChunk {
                size,
                common: None,
                sound: None,
                chunks: vec![],
            }),
            ids::AIFF_C => {
                println!("aiff c file detected");
                Err(ChunkError::InvalidFormType(form_type))
            }
            &x => Err(ChunkError::InvalidFormType(x)),
        }
    }
}

struct CommonChunk {
    pub size: i32,
    pub num_channels: i16,
    pub num_sample_frames: u32,
    pub sample_size: i16,     // AKA bit depth
    pub sample_rate: Decimal, // 80 bit extended floating pt num
}

impl Chunk for CommonChunk {
    fn build(cb: ChunkBuilder, buf: Buffer) -> Result<CommonChunk, ChunkError> {
        if cb.id() != ids::COMMON {
            return Err(ChunkError::InvalidID(cb.consume()));
        }

        let (size, num_channels, num_sample_frames, sample_size) = (
            buf.get_i32_be(),
            buf.get_i16_be(),
            buf.get_u32_be(),
            buf.get_i16_be(),
        );

        // rust_decimal requires 96 bits to create a decimal
        // the extended precision / double long sample rate is 80 bits
        let mut rate_low = [0; 4];
        let mut rate_mid = [0; 4];
        let mut rate_hi = [0; 4];

        buf.copy_to_slice(&mut rate_hi[2..]);
        buf.copy_to_slice(&mut rate_mid);
        buf.copy_to_slice(&mut rate_low);

        // FIXME not really sure if this is correct
        let sample_rate = Decimal::from_parts(
            u32::from_le_bytes(rate_low),
            u32::from_le_bytes(rate_mid),
            u32::from_le_bytes(rate_hi),
            false,
            23,
        );

        Ok(CommonChunk {
            size,
            num_channels,
            num_sample_frames,
            sample_size,
            sample_rate,
        })
    }
}

struct SoundDataChunk {
    pub size: i32,
    pub offset: u32,
    pub block_size: u32,
    pub sound_data: Vec<u8>,
}

impl Chunk for SoundDataChunk {
    fn build(
        cb: ChunkBuilder,
        buf: Buffer,
    ) -> Result<SoundDataChunk, ChunkError> {
        // A generic for the tag check would be nice
        if cb.id() != ids::SOUND {
            return Err(ChunkError::InvalidID(cb.consume()));
        }

        let size = buf.get_i32_be();
        let offset = buf.get_u32_be();
        let block_size = buf.get_u32_be();

        // TODO compare size / sound size with CommonChunk::num_sample_frames
        // size should be equal to num_sample_frames * num_channels.
        // According to the spec, `size` should account for offset + block_size + sound_data
        // or at least, it's implied? Either way, accounting for it causes current output
        // to make less sense.

        // let sound_size = size - 8; // offset + blocksize = 8 bytes
        let sound_size = size;
        let start = buf.position() as usize;
        let stop = start + sound_size as usize;

        //        let mut sound_data = Vec::from(&buf.get_mut()[start..stop]);
        //        buf.advance(sound_size as usize);

        // TODO compare these methods

        let mut sound_data = vec![0; sound_size as usize];
        buf.copy_to_slice(&mut sound_data);

        Ok(SoundDataChunk {
            size,
            offset,
            block_size,
            sound_data,
        })
    }
}

// TODO testme with pascal strings
pub fn read_pstring(buf: Buffer) -> String {
    let len = buf.get_u8();
    let mut str_buf = vec![];

    for _ in 0..len {
        str_buf.push(buf.get_u8());
    }

    String::from_utf8(str_buf).unwrap()
}

struct Marker {
    id: i16,
    position: u32,
    marker_name: String,
}

struct MarkerChunk {
    pub size: i32,
    pub num_markers: u16,
    pub markers: Vec<Marker>,
}

enum TextChunkType {
    Name,
    Author,
    Copyright,
    Annotation,
}

struct TextChunk {
    chunk_type: TextChunkType,
    size: i32,
    text: String,
}

impl Chunk for TextChunk {
    fn build(cb: ChunkBuilder, buf: Buffer) -> Result<TextChunk, ChunkError> {
        let chunk_type = match cb.id() {
            ids::NAME => TextChunkType::Name,
            ids::AUTHOR => TextChunkType::Author,
            ids::COPYRIGHT => TextChunkType::Copyright,
            ids::ANNOTATION => TextChunkType::Annotation,
            _ => return Err(ChunkError::InvalidID(cb.consume())),
        };

        // FIXME copy slice
        let size = buf.get_i32_be();
        let mut text_bytes = vec![];
        for _ in 0..size {
            text_bytes.push(buf.get_u8());
        }
        let text = String::from_utf8(text_bytes).unwrap();

        Ok(TextChunk {
            chunk_type,
            size,
            text,
        })
    }
}

// rust-id3 is a far better library - the best option is probably to return
// a generic u8 array that is compatible with that library. we can read the
// tag and size, and cut a slice to size to extract it. for now, we only
// cut the slice to know where the data is.
struct ID3Chunk {
    version: [u8; 2],
}

// IMPORTANT - there is a COMM ID here as well. not a a problem
// if the id3 data is separated.
impl Chunk for ID3Chunk {
    fn build(cb: ChunkBuilder, buf: Buffer) -> Result<ID3Chunk, ChunkError> {
        let id = cb.id();
        if &id[0..3] != ids::ID3 {
            return Err(ChunkError::InvalidID(cb.consume()));
        }

        let version = [id[3], buf.get_u8()];

        match version {
            [2, 0] => println!("id3 v2.0-2.2"),
            [3, 0] => println!("id3 v2.3"),
            [4, 0] => println!("id3 v2.4"),
            x => {
                println!("unknown version {:?}", x);
                return Err(ChunkError::InvalidID3Version(x));
            }
        }

        // TODO check bit flags
        let flags = buf.get_u8();
        if flags != 0 {
            println!(
                "flags were set; currently unable to parse flags: {}",
                flags
            );
        }

        // "The ID3v2 tag size is encoded with four bytes where the most
        // significant bit (bit 7) is set to zero in every byte, making a total
        // of 28 bits. The zeroed bits are ignored, so a 257 bytes long tag is
        // represented as $00 00 02 01." - http://id3.org/id3v2.3.0
        let (s1, s2, s3, s4) =
            (buf.get_u8(), buf.get_u8(), buf.get_u8(), buf.get_u8());
        println!(
            "size bits s1 {} s2 {} s3 {} s4 {} remaining {}",
            s1,
            s2,
            s3,
            s4,
            buf.remaining()
        );

        while buf.remaining() >= 4 {
            let mut id = [0; 4];
            buf.copy_to_slice(&mut id);

            let mut size = buf.get_u32_be();

            let mut flags = [0; 2];
            buf.copy_to_slice(&mut flags);

            let mut data = vec![0; size as usize];
            buf.copy_to_slice(&mut data);

            println!(
                "id {} size {} remaining {}",
                String::from_utf8_lossy(&id),
                size,
                buf.remaining()
            )
        }

        Ok(ID3Chunk { version })
    }
}