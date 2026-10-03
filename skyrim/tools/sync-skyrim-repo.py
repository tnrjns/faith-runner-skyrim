"""Copies the Skyrim side of faith-runner into its own repository folder (faith-runner-skyrim).
Run again to refresh it; only the listed files are copied, nothing is deleted outside them."""
import os
import shutil

SRC = r'C:\Users\myste\Downloads\faith-runner\faith-runner'
DST = r'C:\Users\myste\Downloads\faith-runner\faith-runner-skyrim'

# Never copied: builds, logs, local tools, the prologue map's code, the unused ESP tool.
SKIP_DIRS = {'target', 'build', '.tools', 'ParkourEsp', 'CommonLibSSE-NG', '.git', 'bin', 'obj'}
SKIP_FILES = {
    # me_assets: the prologue map (not public)
    'level.rs', 'collision.rs', 'staticmesh.rs', 'postfx.rs', 'material.rs', 'level_tests.rs',
}
SKIP_SUFFIX = ('.log',)

TREES = [
    'crates/faith_move',
    'crates/faith_anim',
    'crates/me_assets',
    'crates/faith_ffi',
    'crates/parkour_tool',
    'skyrim',
]


def copy_tree(rel):
    src_root = os.path.join(SRC, rel)
    for dirpath, dirnames, filenames in os.walk(src_root):
        dirnames[:] = [d for d in dirnames if d not in SKIP_DIRS]
        for f in filenames:
            if f.endswith(SKIP_SUFFIX):
                continue
            if rel == 'crates/me_assets' and f in SKIP_FILES:
                continue
            s = os.path.join(dirpath, f)
            d = os.path.join(DST, os.path.relpath(s, SRC))
            os.makedirs(os.path.dirname(d), exist_ok=True)
            shutil.copy2(s, d)


os.makedirs(DST, exist_ok=True)
for t in TREES:
    copy_tree(t)
shutil.copy2(os.path.join(SRC, 'Cargo.lock'), os.path.join(DST, 'Cargo.lock'))
print('copied')
