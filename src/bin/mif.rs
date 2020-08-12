//! `mif` binary.

#![forbid(unsafe_code)]
#![forbid(missing_docs)]

use std::{path::PathBuf, io::stdout};
use clap::{crate_version, crate_authors, Clap, AppSettings};
use anyhow::Result;
use mif::{First, cli::{open, dump, load, join}};
use Mif::{Dump, Join};

/// Memory Initialization File.
#[derive(Clap)]
#[clap(
	version = crate_version!(),
	author = crate_authors!(),
	max_term_width = 80,
	global_setting = AppSettings::ColoredHelp,
	global_setting = AppSettings::DeriveDisplayOrder,
	global_setting = AppSettings::UnifiedHelpMessage,
	global_setting = AppSettings::ArgRequiredElseHelp,
)]
enum Mif {
	/// Dumps binary as MIF.
	Dump {
		/// Input file or standard input (-).
		#[clap(default_value = "-")]
		input: PathBuf,
		/// Word width in bits from 1 to 128.
		#[clap(short = "w", long = "width", value_name = "bits")]
		#[clap(default_value = "16")]
		width: usize,
		/// LSB/MSB first (little/big-endian).
		#[clap(short = "f", long = "first", value_name = "lsb|msb")]
		#[clap(default_value = "lsb")]
		first: First,
	},
	/// Joins binaries' memory areas to MIFs.
	Join {
		/// TOML file or standard input (-).
		#[clap(default_value = "-")]
		toml: PathBuf,
		/// Input directory [default: .].
		#[clap(short = "i", long = "bins", value_name = "path")]
		bins: Option<PathBuf>, // `default_value = ""` broken for non-pos opts.
		/// Output directory [default: .].
		#[clap(short = "o", long = "mifs", value_name = "path")]
		mifs: Option<PathBuf>, // `default_value = ""` broken for non-pos opts.
		/// No comments in MIFs.
		#[clap(short = "n", long = "no-comments")]
		nocs: bool,
	},
}

fn main() -> Result<()> {
	match Mif::parse() {
		Dump { input, width, first } => {
			let (mut bytes, count) = open(&input)?;
			dump(&mut stdout(), &mut bytes, count, width, first)
		},
		Join { toml, bins, mifs, nocs } => {
			let bins = bins.unwrap_or_default();
			let mifs = mifs.unwrap_or_default();
			join(&load(&toml)?, (&bins, &mifs), !nocs)
		},
	}
}
