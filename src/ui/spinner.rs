const FRAMES: [&str; 6] = ["[=   ]", "[ =  ]", "[  = ]", "[   =]", "[  = ]", "[ =  ]"];

pub(crate) fn frame(tick: u64) -> &'static str {
    FRAMES[tick as usize % FRAMES.len()]
}
