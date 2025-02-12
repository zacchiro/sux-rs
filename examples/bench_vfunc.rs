/*
 *
 * SPDX-FileCopyrightText: 2023 Sebastiano Vigna
 *
 * SPDX-License-Identifier: Apache-2.0 OR LGPL-2.1-or-later
 */

use anyhow::Result;
use clap::Parser;
use dsi_progress_logger::*;
use epserde::prelude::*;
use sux::func::VFunc;

#[derive(Parser, Debug)]
#[command(about = "Benchmark VFunc with strings or 64-bit integers", long_about = None)]
struct Args {
    #[arg(short = 'f', long)]
    /// A file containing UTF-8 keys, one per line. If not specified, the 64-bit keys [0..n) are used.
    filename: Option<String>,
    #[arg(short)]
    /// The maximum number strings to use from the file, or the number of 64-bit keys.
    n: usize,
    /// A name for the ε-serde serialized function with u64 keys.
    func: String,
}

fn main() -> Result<()> {
    stderrlog::new()
        .verbosity(2)
        .timestamp(stderrlog::Timestamp::Second)
        .init()
        .unwrap();

    let mut pl = ProgressLogger::default();

    let args = Args::parse();

    if let Some(filename) = args.filename {
        let func = VFunc::<_>::load_mem(&args.func)?;
        let keys = sux::utils::file::FilenameIntoIterator(&filename)
            .into_iter()
            .take(args.n)
            .collect::<Vec<_>>();
        pl.start("Querying...");
        for (i, key) in keys.iter().enumerate() {
            assert_eq!(i, func.get(key));
        }
        pl.done_with_count(keys.len());
    } else {
        let func = VFunc::<_>::load_mem(&args.func)?;
        pl.start("Querying...");
        for i in 0..args.n {
            assert_eq!(i, func.get(&i));
        }
        pl.done_with_count(args.n);
    }

    Ok(())
}
