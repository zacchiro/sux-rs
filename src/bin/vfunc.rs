/*
 *
 * SPDX-FileCopyrightText: 2023 Sebastiano Vigna
 *
 * SPDX-License-Identifier: Apache-2.0 OR LGPL-2.1-or-later
 */

use anyhow::Result;
use clap::{ArgGroup, Parser};
use dsi_progress_logger::*;
use epserde::ser::Serialize;
use sux::prelude::VFuncBuilder;
use sux::utils::file::FilenameIntoIterator;
use sux::utils::FilenameZstdIntoIterator;

#[derive(Parser, Debug)]
#[command(about = "Generate a VFunc mapping each input to its rank and serialize it with ε-serde", long_about = None)]
#[clap(group(
            ArgGroup::new("input")
                .required(true)
                .args(&["filename", "n"]),
))]
struct Args {
    #[arg(short, long)]
    /// A file containing UTF-8 keys, one per line.
    filename: Option<String>,
    #[arg(short)]
    /// Use the 64-bit keys [0..n). Mainly useful for testing and debugging.
    n: Option<usize>,
    /// Use this number of threads.
    #[arg(short, long)]
    threads: Option<usize>,
    /// A name for the ε-serde serialized function.
    func: String,
    /// The filename containing the keys is compressed with zstd.
    #[arg(short, long)]
    zstd: bool,
    /// Use disk-based buckets to reduce memory usage at construction time.
    #[arg(short, long)]
    offline: bool,
    /// The number of high bits defining the number of buckets. Very large key sets may benefit from a larger number of buckets.
    #[arg(short, long, default_value_t = 8)]
    high_bits: u32,
}

fn main() -> Result<()> {
    stderrlog::new()
        .verbosity(2)
        .timestamp(stderrlog::Timestamp::Second)
        .init()
        .unwrap();

    let args = Args::parse();

    let mut pl = ProgressLogger::default();
    pl.display_memory(true);

    if let Some(filename) = args.filename {
        let mut builder = VFuncBuilder::default()
            .offline(args.offline)
            .log2_buckets(args.high_bits);
        if let Some(threads) = args.threads {
            builder = builder.num_threads(threads);
        }
        let func = if args.zstd {
            builder.build(FilenameZstdIntoIterator(&filename), &(0_usize..), &mut pl)?
        } else {
            builder.build(FilenameIntoIterator(&filename), &(0..), &mut pl)?
        };
        func.store(&args.func)?;
    }

    if let Some(n) = args.n {
        let mut builder = VFuncBuilder::default()
            .offline(args.offline)
            .log2_buckets(args.high_bits);
        if let Some(threads) = args.threads {
            builder = builder.num_threads(threads);
        }
        let func = builder.build(0..n, &(0_usize..), &mut pl)?;

        func.store(&args.func)?;
    }
    Ok(())
}
