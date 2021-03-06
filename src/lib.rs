//! Memory Initialization File
//!
//! # Features
//!
//!   * Native MIF representation as `Vec<(word: T, bulk: usize)>`.
//!   * Detects single-word sequence `[first..last]: word` but does **not**
//!     detect multi-word sequence `[first..last]: words..` in binary data.
//!   * Verifies word fits into MIF's word width in bits.
//!   * Joins multiple MIFs of different word widths as long as words fit.
//!   * Optionally comments join offsets in words with given (file) names.
//!   * Provides simple `mif dump` subcommand.
//!   * Provides reproducible `mif join` subcommand via TOML instruction file.
//!
//! # Library
//!
//! MIF creation and serialization is implemented for the `Mif` structure.
//!
//! Disable default features like `cli` and `bin` to reduce dependencies:
//!
//! ```toml
//! [dependencies]
//! mif = { version = "0.3", default-features = false }
//! ```
//!
//! Default features:
//!
//!   * `cli`: Provides command-line interface functionality of `mif` binary.
//!
//!     Requires: `anyhow`, `indexmap`, `serde`, `toml`
//!
//!   * `bin`: Enables compilation of `mif` binary.
//!
//!     Requires: `cli`, `clap`
//!
//! # Command-line Interface
//!
//! Install via `cargo install mif`.
//!
//! Provides two subcommands, `dump` and `join`.
//!
//! ```text
//! mif 0.3.0
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
//!     -w, --width <bits>       Word width in bits from 1 to 128 [default: 16]
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

/// Command-line interface functionality of `mif` binary.
#[cfg(feature = "cli")]
pub mod cli;
#[cfg(feature = "cli")]
use serde::Deserialize;

use std::{
	mem::size_of,
	path::PathBuf,
	io::{self, Read, Write},
	result,
	fmt::UpperHex,
	str::FromStr,
};
use num_traits::{
	sign::Unsigned, int::PrimInt, cast::FromPrimitive,
	ops::{checked::CheckedShl, wrapping::WrappingSub},
};
use byteorder::{LE, BE, ReadBytesExt};
use thiserror::Error;
use First::{Lsb, Msb};
use Error::*;

type Result<T> = result::Result<T, Error>;

/// `Mif` errors.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum Error {
	/// Neither `"lsb"` nor `"msb"` first.
	#[error("Valid values are `lsb` and `msb`")]
	NeitherLsbNorMsbFirst,
	/// Width exceeds `[1, Mif::max_width()]`
	#[error("Width {0} out of [1, {1}]")]
	WidthOutOfRange(usize, usize),
	/// Word value exceeds `Mif::max_value()`.
	#[error("Word at depth {0} out of width {1}")]
	ValueOutOfWidth(usize, usize),
	/// Less words read than expected.
	#[error("Missing {0} words")]
	MissingWords(usize),
	/// I/O error.
	#[error(transparent)]
	IoError(#[from] io::Error),
}

/// Native MIF representation.
#[derive(Debug, Eq, PartialEq, Clone)]
pub struct Mif<T: UpperHex + Unsigned + PrimInt + FromPrimitive> {
	width: usize,
	depth: usize,
	words: Vec<(T, usize)>,
	areas: Vec<(usize, PathBuf)>,
}

impl<T> Mif<T>
where
	T: UpperHex + Unsigned + PrimInt + FromPrimitive + CheckedShl + WrappingSub,
{
	/// Creates new MIF with word `width`.
	pub fn new(width: usize) -> Result<Mif<T>> {
		if (1..=Self::max_width()).contains(&width) {
			Ok(Mif { words: Vec::new(), depth: 0, areas: Vec::new(), width })
		} else {
			Err(WidthOutOfRange(width, Self::max_width()))
		}
	}
	/// Maximum word width in bits depending on `T`.
	pub fn max_width() -> usize {
		Self::max_align() * 8
	}
	/// Maximum word width in bytes depending on `T`.
	pub fn max_align() -> usize {
		size_of::<T>()
	}
	/// Maximum word value depending on `width()`.
	pub fn max_value(&self) -> T {
		T::one().checked_shl(self.width as u32)
			.unwrap_or(T::zero()).wrapping_sub(&T::one())
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
		match self.words.last_mut() {
			Some((last_word, last_bulk)) if *last_word == word =>
				*last_bulk += bulk,
			_ => {
				if word > self.max_value() {
					Err(ValueOutOfWidth(self.depth, self.width()))?;
				}
				if bulk > 0 {
					self.words.push((word, bulk))
				}
			},
		}
		self.depth += bulk;
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
				.ok_or(ValueOutOfWidth(words, self.width))?, 1)?;
			words += 1;
		}
		if depth != words {
			Err(MissingWords(depth - words))?;
		}
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

/// LSB/MSB first (little/big-endian).
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
#[cfg_attr(feature = "cli", derive(Deserialize))]
#[cfg_attr(feature = "cli", serde(rename_all = "kebab-case"))]
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
			_ => Err(NeitherLsbNorMsbFirst),
		}
	}
}

/// Default width of 16 bits.
pub const fn default_width() -> usize { 16 }
