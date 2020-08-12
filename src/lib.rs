//! Memory Initialization File
//!
//! # Library
//!
//! MIF creation and serialization is implemented via the `Mif` structure.
//!
//! Disable default features like `cli` to reduce dependencies:
//!
//! ```toml
//! [dependencies]
//! mif = { version = "0.1", default-features = false }
//! ```
//!
//! # Command-line Interface
//!
//! Provides two subcommands, `dump` and `join`.
//!
//! ```text
//! mif 0.1.1
//! Rouven Spreckels <rs@qu1x.dev>
//! Memory Initialization File
//!
//! USAGE:
//!     mif <SUBCOMMAND>
//!
//! OPTIONS:
//!     -h, --help       Prints help information
//!     -V, --version    Prints version information
//!
//! SUBCOMMANDS:
//!     dump    Dumps binary as MIF
//!     join    Joins binaries' memory areas to MIFs
//!     help    Prints this message or the help of the given subcommand(s)
//! ```
//!
//! ## Dump Subcommand
//!
//! ```text
//! mif-dump
//! Dumps binary as MIF
//!
//! USAGE:
//!     mif dump [input]
//!
//! ARGS:
//!     <input>    Input file or standard input (-) [default: -]
//!
//! OPTIONS:
//!     -w, --width <bits>       Word width in bits from 8 to 128 [default: 16]
//!     -f, --first <lsb|msb>    LSB/MSB first (little/big-endian) [default: lsb]
//!     -h, --help               Prints help information
//!     -V, --version            Prints version information
//! ```
//!
//! ## Join Subcommand
//!
//! ```text
//! mif-join
//! Joins binaries' memory areas to MIFs
//!
//! USAGE:
//!     mif join [OPTIONS] [toml]
//!
//! ARGS:
//!     <toml>    TOML file or standard input (-) [default: -]
//!
//! OPTIONS:
//!     -i, --bins <path>    Input directory [default: .]
//!     -o, --mifs <path>    Output directory [default: .]
//!     -n, --no-comments    No comments in MIFs
//!     -h, --help           Prints help information
//!     -V, --version        Prints version information
//! ```
//!
//! ### Join Example
//!
//! Assuming two ROM dumps, `a.rom` and `b.rom`, whose program and data areas
//! are concatenated as in:
//!
//!   * `cat a.program.rom a.data.rom > a.rom`
//!   * `cat b.program.rom b.data.rom > b.rom`
//!
//! Following TOML file defines how to join both program areas to one MIF and
//! both data areas to another MIF, assuming 24-bit program words of depth 1267
//! and 1747 and 16-bit data words of depth 1024 each. Additionally, every area
//! is dumped to its own separate MIF for verification. Then, between program
//! and data area is supposed to be an unused area of `0xffffff` words, which
//! should be skipped. Listing them in the `skips` instruction will verify that
//! this area only contains these words.
//!
//! ```toml
//! [["a.rom"]]
//! first = "lsb" # Least-significant byte first. Default, can be omitted.
//! width = 24
//! depth = 1267
//! joins = ["a.prog.mif", "ab.prog.mif"]
//! [["a.rom"]]
//! first = "lsb" # Least-significant byte first. Default, can be omitted.
//! width = 24
//! depth = 781
//! skips = [0xffffff] # Empty [] for skipping without verification.
//! [["a.rom"]]
//! first = "msb"
//! width = 16 # Default, can be omitted.
//! depth = 1024
//! joins = ["a.data.mif", "ab.data.mif"]
//!
//! [["b.rom"]]
//! width = 24
//! depth = 1747
//! joins = ["b.prog.mif", "ab.prog.mif"]
//! [["b.rom"]]
//! width = 24
//! depth = 301
//! skips = [0xffffff]
//! [["b.rom"]]
//! depth = 1024
//! joins = ["b.data.mif", "ab.data.mif"]
//! ```

#![forbid(unsafe_code)]
#![forbid(missing_docs)]

use std::{
	convert::TryInto,
	path::{PathBuf, Path},
	fs::{OpenOptions, metadata},
	io::{Cursor, BufReader, Read, stdin, BufWriter, Write},
	fmt::UpperHex,
	str::FromStr,
};
use serde::Deserialize;
use indexmap::IndexMap;
use num_traits::{int::PrimInt, cast::FromPrimitive};
use byteorder::{LE, BE, ReadBytesExt};
use anyhow::{Error, Result, Context, anyhow, ensure};
use First::*;
use Instr::*;

/// Opens file or standard input `"-"` as buffered bytes reader of known count.
///
///   * For a file, the count is determined by `metadata()`.
///   * For standard input, the bytes are completely read in and counted.
pub fn open(input: &dyn AsRef<Path>) -> Result<(Box<dyn Read>, usize)> {
	let input = input.as_ref();
	Ok(if input == Path::new("-") {
		let mut bytes = Vec::new();
		stdin().read_to_end(&mut bytes).context("Cannot read standard input")?;
		let count = bytes.len();
		(Box::new(Cursor::new(bytes)), count)
	} else {
		let (bytes, count) = OpenOptions::new().read(true).open(&input)
			.and_then(|bytes| metadata(&input)
				.map(|stats| (BufReader::new(bytes), stats.len())))
			.with_context(|| format!("Cannot open `{}`", input.display()))?;
		(Box::new(bytes), count.try_into().context("Address space exhausted")?)
	})
}

/// Dumps known count of bytes from reader as MIF to writer.
///
///   * `lines`: Writer, MIF is written to.
///   * `bytes`: Reader, bytes are read from.
///   * `count`: Count of bytes to read.
///   * `width`: Word width in bits from 8 to 128.
///   * `first`: LSB/MSB first (little/big-endian).
pub fn dump(
	lines: &mut dyn Write,
	bytes: &mut dyn Read,
	count: usize,
	width: usize,
	first: First,
) -> Result<()> {
	let mut mif = Mif::<u128>::new(width)?;
	let align = mif.align();
	let depth = count / align;
	ensure!(depth * align == count, "No integral multiple of word width");
	mif.read(bytes, depth, first).context("Cannot read input")
		.and_then(|()| mif.write(lines, false).context("Cannot write MIF"))
}

/// Load TOML from file or standard input `"-"` as `Files`.
#[cfg(feature = "cli")]
pub fn load(input: &dyn AsRef<Path>) -> Result<Files> {
	let input = input.as_ref();
	let mut file: Box<dyn Read> = if input == Path::new("-") {
		Box::new(stdin())
	} else {
		Box::new(OpenOptions::new().read(true).open(&input).map(BufReader::new)
			.with_context(|| format!("Cannot open `{}`", input.display()))?)
	};
	let mut string = String::new();
	file.read_to_string(&mut string)
		.with_context(|| format!("Cannot read `{}`", input.display()))
	.and_then(|_count| toml::from_str::<Files>(&string)
		.with_context(|| format!("Cannot load `{}`", input.display())))
}

/// Joins memory areas of binary `Files` as MIFs.
///
///   * `files`: Binary files split into memory areas, see `Files`.
///   * `areas`: Whether to comment memory areas, see `write()`.
///   * `paths`: Prefix paths for input binaries and output MIFs in given order.
pub fn join(
	files: &Files,
	paths: (&dyn AsRef<Path>, &dyn AsRef<Path>),
	areas: bool,
) -> Result<()> {
	let mut mifs = IndexMap::new();
	for (bin_path, areas) in files {
		let mut abs_path = paths.0.as_ref().to_path_buf();
		abs_path.push(&bin_path);
		let mut bin_file = OpenOptions::new()
			.read(true).open(&abs_path).map(BufReader::new)
			.with_context(|| format!("Cannot open `{}`", abs_path.display()))?;
		for &Area { first, width, depth, ref instr } in areas {
			let mut mif_area = Mif::new(width)?;
			mif_area.read(&mut bin_file, depth, first)?;
			match instr {
				Skips(skips) => if !skips.is_empty() {
					ensure!(mif_area.words().iter()
						.all(|&(word, _bulk)| skips.iter()
							.any(|skip| skip.as_word() == word)),
						"Invalid word to skip in `{}`", bin_path.display());
				},
				Joins(joins) => for mif_path in joins {
					if !mifs.contains_key(mif_path) {
						let mut abs_path = paths.1.as_ref().to_path_buf();
						abs_path.push(mif_path);
						let mif_file = OpenOptions::new()
							.write(true).create(true).truncate(true)
							.open(&abs_path).map(BufWriter::new)
							.with_context(|| format!("Cannot open `{}`",
								abs_path.display()))?;
						let mif = (mif_file, Mif::new(width)?);
						assert!(mifs.insert(mif_path.clone(), mif).is_none());
					}
					let (_mif_file, mif_data) = &mut mifs[mif_path];
					ensure!(mif_data.width() == width,
						"Different width to join `{}`", mif_path.display());
					mif_data.area(bin_path.clone());
					mif_data.join(&mif_area)?;
				},
			}
		}
		let mut bin_data = Vec::new();
		bin_file.read_to_end(&mut bin_data)?;
		ensure!(bin_data.is_empty(),
			"{} B left over in `{}`", bin_data.len(), bin_path.display());
	}
	for (mif_path, (mut mif_file, mif_data)) in mifs {
		mif_data.write(&mut mif_file, areas)
			.with_context(|| format!("Cannot write `{}`", mif_path.display()))?;
	}
	Ok(())
}

/// Native MIF representation.
#[derive(Debug, Eq, PartialEq, Clone)]
pub struct Mif<T: UpperHex + PrimInt + FromPrimitive> {
	width: usize,
	depth: usize,
	words: Vec<(T, usize)>,
	areas: Vec<(usize, PathBuf)>,
}

impl<T: UpperHex + PrimInt + FromPrimitive> Mif<T> {
	/// Creates new MIF with word `width`.
	pub fn new(width: usize) -> Result<Mif<T>> {
		ensure!((8..=128).contains(&width),
			"Word width {} out of [8, 128]", width);
		Ok(Mif { words: Vec::new(), depth: 0, areas: Vec::new(), width })
	}
	/// Word width in bits.
	pub fn width(&self) -> usize {
		self.width
	}
	/// Word width in bytes.
	pub fn align(&self) -> usize {
		(self.width as f64 / 8.0).ceil() as usize
	}
	/// MIF depth in words.
	pub fn depth(&self) -> usize {
		self.depth
	}
	/// Reference to words and their bulk in given order.
	pub fn words(&self) -> &Vec<(T, usize)> {
		&self.words
	}
	/// Reference to addresses and paths of memory areas in given order.
	pub fn areas(&self) -> &Vec<(usize, PathBuf)> {
		&self.areas
	}
	/// Addresses memory `area` at current `depth()`.
	pub fn area(&mut self, area: PathBuf) {
		self.areas.push((self.depth, area));
	}
	/// Pushes `word` or add up its `bulk`.
	pub fn push(&mut self, word: T, bulk: usize) -> Result<()> {
		self.depth += bulk;
		match self.words.last_mut() {
			Some((last_word, last_bulk)) if *last_word == word =>
				*last_bulk += bulk,
			_ => {
				ensure!(word < T::one().unsigned_shl(self.width as u32),
					"Word exceeds width");
				if bulk > 0 {
					self.words.push((word, bulk))
				}
			},
		}
		Ok(())
	}
	/// Joins in `other` MIF.
	pub fn join(&mut self, other: &Self) -> Result<()> {
		other.words.iter().try_for_each(|&(word, bulk)| self.push(word, bulk))
	}
	/// Reads `depth` LSB/MSB-`first` words from `bytes` reader.
	pub fn read(&mut self, bytes: &mut dyn Read, depth: usize, first: First)
	-> Result<()> {
		let align = self.align();
		let mut words = 0;
		for _ in 0..depth {
			let word = match first {
				Lsb => bytes.read_uint128::<LE>(align),
				Msb => bytes.read_uint128::<BE>(align),
			}?;
			self.push(T::from_u128(word)
				.ok_or(anyhow!("Word larger than width"))?, 1)?;
			words += 1;
		}
		ensure!(depth == words, "Not enough words");
		Ok(())
	}
	/// Writes MIF to writer.
	///
	///   * `lines`: Writer, MIF is written to.
	///   * `areas`: Whether to comment memory areas as in `-- 0000: name.bin`.
	pub fn write(&self, lines: &mut dyn Write, areas: bool) -> Result<()> {
		let addr_pads = (self.depth as f64).log(16.0).ceil() as usize;
		let word_pads = (self.width as f64 / 4.0).ceil() as usize;
		if areas && !self.areas.is_empty() {
			for (addr, path) in &self.areas {
				writeln!(lines, "-- {:02$X}: {}",
					addr, path.display(), addr_pads)?;
			}
			writeln!(lines)?;
		}
		writeln!(lines, "\
			WIDTH={};\n\
			DEPTH={};\n\
			\n\
			ADDRESS_RADIX=HEX;\n\
			DATA_RADIX=HEX;\n\
			\n\
			CONTENT BEGIN", self.width, self.depth)?;
		let mut addr = 0;
		for &(word, bulk) in &self.words {
			if bulk == 1 {
				writeln!(lines, "\t{:02$X}  :   {:03$X};",
					addr, word, addr_pads, word_pads)?;
			} else {
				writeln!(lines, "\t[{:03$X}..{:03$X}]  :   {:04$X};",
					addr, addr + bulk - 1, word, addr_pads, word_pads)?;
			}
			addr += bulk;
		}
		writeln!(lines, "END;")?;
		Ok(())
	}
}

/// Binary files split into memory areas.
pub type Files = IndexMap<PathBuf, Vec<Area>>;

/// Memory area.
#[derive(Debug, Eq, PartialEq, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Area {
	/// LSB/MSB first (little/big-endian).
	#[serde(default)]
	pub first: First,
	/// Word width in bits from 8 to 128.
	#[serde(default = "default_width")]
	pub width: usize,
	/// Depth in words.
	pub depth: usize,
	/// Whether to skip or join this memory area.
	#[serde(flatten)]
	pub instr: Instr,
}

const fn default_width() -> usize { 16 }

/// LSB/MSB first (little/big-endian).
#[derive(Debug, Eq, PartialEq, Copy,Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum First {
	/// Least-significant byte first (little-endian).
	Lsb,
	/// Most-significant byte first (big-endian).
	Msb,
}

impl Default for First {
	fn default() -> Self { Lsb }
}

impl FromStr for First {
	type Err = Error;

	fn from_str(from: &str) -> Result<Self> {
		match from {
			"lsb" => Ok(Lsb),
			"msb" => Ok(Msb),
			_ => Err(anyhow!("Valid values are `lsb` and `msb`")),
		}
	}
}

/// Whether to skip or join a memory area.
#[derive(Debug, Eq, PartialEq, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Instr {
	/// Skips memory area and ensures it contains given words only.
	Skips(Vec<Word>),
	/// Joins memory area to given MIFs.
	Joins(Vec<PathBuf>),
}

/// TOML `u128` workaround.
#[derive(Debug, Eq, PartialEq, Copy, Clone, Deserialize)]
#[serde(untagged)]
pub enum Word {
	/// One `u64` TOML integer as `u128`.
	One(u64),
	/// Two `u64` TOML integers `[msb, lsb]` as `u128`.
	Two([u64; 2])
}

impl Word {
	fn as_word(&self) -> u128 {
		match *self {
			Word::One(one) => one as u128,
			Word::Two(two) => (two[0] as u128) << 64 | two[1] as u128,
		}
	}
}
