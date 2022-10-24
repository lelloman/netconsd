/*
 * Copyright (C) 2022, Meta, Inc.
 * All rights reserved.
 *
 * This source code is licensed under the license found in the LICENSE file in
 * the root directory of this source tree.
 */

use netconsd_module::c_int;
use netconsd_module::in6_addr;
use netconsd_module::MsgBuf;
use netconsd_module::NcrxMsg;

mod stats_holder;
mod stats_printer;

use stats_holder::get_stats_holder;
use stats_holder::init_stats_holder;
use stats_printer::exit_stats_printer;
use stats_printer::init_stats_printer;

pub fn netconsd_output_init(nr_workers: c_int) -> c_int {
    println!("Stats module init ({} workers)", nr_workers);
    init_stats_holder(nr_workers as usize);
    init_stats_printer();
    0
}

pub fn netconsd_output_handler(
    t: c_int,
    in6_addr: *const in6_addr,
    buf: *const MsgBuf,
    msg: *const NcrxMsg,
) -> i32 {
    get_stats_holder().on_new_message(t as usize, in6_addr, buf, msg);
    0
}

pub fn netconsd_output_exit() {
    exit_stats_printer();
    println!("Unload stats module.");
}
