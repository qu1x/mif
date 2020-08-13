#![forbid(unsafe_code)]
#![forbid(missing_docs)]

use std::{
	convert::TryInto,
	path::{PathBuf, Path},
	fs::{OpenOptions, metadata},
	io::{Cursor, BufReader, Read, stdin, BufWriter, Write},
};
use serde::Deserialize;
use indexmap::IndexMap;
use anyhow::{Result, Context, ensure};
use Instr::{Skips, Joins};
use crate::{Mif, First, default_width};

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
///   * `width`: Word width in bits from 1 to 128.
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
///   * `paths`: Prefix paths for input binaries and output MIFs in given order.
///   * `areas`: Whether to comment memory areas, see `write()`.
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

/// Binary files split into memory areas.
pub type Files = IndexMap<PathBuf, Vec<Area>>;

/// Memory area.
#[derive(Debug, Eq, PartialEq, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Area {
	/// LSB/MSB first (little/big-endian).
	#[serde(default)]
	pub first: First,
	/// Word width in bits from 1 to 128.
	#[serde(default = "default_width")]
	pub width: usize,
	/// Depth in words.
	pub depth: usize,
	/// Whether to skip or join this memory area.
	#[serde(flatten)]
	pub instr: Instr,
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
