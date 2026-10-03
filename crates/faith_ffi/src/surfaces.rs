//! What Faith's feet and hands touch in the host's world, for her footstep and handstep sounds.

use faith_move::greybox::Surface;

use crate::{guard, handle, Faith};

fn surface(n: u32) -> Surface {
    match n {
        1 => Surface::Wood,
        2 => Surface::Metal,
        3 => Surface::MetalGantry,
        4 => Surface::MetalLadder,
        5 => Surface::MetalFence,
        6 => Surface::MetalPipe,
        7 => Surface::Airduct,
        8 => Surface::Cardboard,
        9 => Surface::Water,
        10 => Surface::Glass,
        _ => Surface::Concrete,
    }
}

/// What her feet and hands are on now: 0 concrete (stone, and anything Mirror's Edge has no
/// sound for), 1 wood, 2 metal, 3 metal grating, 4 metal ladder, 5 chain-link, 6 metal pipe,
/// 7 airduct, 8 cardboard, 9 water, 10 glass.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_set_surfaces(h: *mut Faith, feet: u32, hands: u32) {
    let Some(f) = (unsafe { handle(h) }) else { return };
    guard((), || f.surfaces = (surface(feet), surface(hands)));
}
