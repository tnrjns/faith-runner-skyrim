/* faith.h: Faith's movement (faith_move) and Mirror's Edge animation (faith_anim) for
 * running inside another game. Link faith_ffi.lib (plus kernel32 ntdll userenv ws2_32
 * dbghelp). Everything is in the host's world frame and units: Z up, `units_per_meter`
 * units a metre, headings in radians clockwise from north (+Y), as Skyrim has them.
 *
 * Single-threaded: call it all from one thread (the game's main thread). */
#pragma once
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define FAITH_API_VERSION 1

typedef struct Faith Faith;

typedef struct FaithVec3 {
    float x, y, z;
} FaithVec3;

/* One frame's controls. */
typedef struct FaithInput {
    float move_x, move_y;       /* strafe right, forward: -1..1 */
    float look_right, look_up;  /* how far the view turned this frame, radians */
    uint8_t jump_pressed, jump_held;
    uint8_t crouch_pressed, crouch_held;
    uint8_t turn_pressed;       /* the 180 turn */
    uint8_t melee_pressed;      /* attack / barge */
    uint8_t takedown_pressed;   /* a takedown on whoever's in front of her (faith_takedowns) */
    uint8_t reaction_pressed; /* Reaction Time: starts it when the meter's full */
} FaithInput;

/* FaithFrame.events */
#define FAITH_EV_JUMP        (1ull << 0)
#define FAITH_EV_LAND        (1ull << 1)
#define FAITH_EV_HARD_LAND   (1ull << 2)
#define FAITH_EV_ROLL        (1ull << 3)
#define FAITH_EV_SLIDE       (1ull << 4)
#define FAITH_EV_WALLRUN     (1ull << 5)
#define FAITH_EV_WALL_JUMP   (1ull << 6)
#define FAITH_EV_WALLCLIMB   (1ull << 7)
#define FAITH_EV_WALL_KICK   (1ull << 8)
#define FAITH_EV_LEDGE_GRAB  (1ull << 9)
#define FAITH_EV_PULL_UP     (1ull << 10)
#define FAITH_EV_VAULT       (1ull << 11)
#define FAITH_EV_MANTLE      (1ull << 12)
#define FAITH_EV_TURN_180    (1ull << 13)
#define FAITH_EV_DODGE       (1ull << 14)
#define FAITH_EV_DEATH       (1ull << 15)
#define FAITH_EV_SPRINGBOARD (1ull << 16)
#define FAITH_EV_MELEE       (1ull << 17)
#define FAITH_EV_BARGE       (1ull << 18)
#define FAITH_EV_MELEE_HIT   (1ull << 19)  /* an attack landed: faith_melee_hits */
#define FAITH_EV_OTHER       (1ull << 31)

/* Where everything is after a step. */
typedef struct FaithFrame {
    FaithVec3 feet;
    FaithVec3 velocity;
    float heading;       /* where you look */
    float pitch;         /* up is positive */
    float body_heading;  /* where the body faces (moves like the wallrun lock it) */
    FaithVec3 cam_pos;
    FaithVec3 cam_forward, cam_up, cam_right;
    float fov_deg;       /* horizontal */
    float land_impact;   /* units/s, with FAITH_EV_LAND */
    uint64_t events;
    uint8_t on_ground;
    uint8_t animated;     /* a Mirror's Edge install was found */
    uint8_t intermediate; /* arms draw in the world's depth this frame (swinging) */
    uint8_t low;          /* crouched or sliding (e.g. sneaking, to the host) */
    float speed_blur;     /* Mirror's Edge's speed blur (TdMotionBlurShader's MotionPacked.r): ~0.5 at full speed */
    float reaction_energy; /* Reaction Time's meter, 0..100 */
    float game_speed;      /* the game speed to run at: 1, down to 0.25 while Reaction Time is on (dt is game time) */
} FaithFrame;

/* Rotation quaternion (x, y, z, w), translation, uniform scale. */
typedef struct FaithXform {
    float rot[4];
    float pos[3];
    float scale;
} FaithXform;

#ifdef __cplusplus
static_assert(sizeof(FaithVec3) == 12, "faith.h layout");
static_assert(sizeof(FaithInput) == 24, "faith.h layout");
static_assert(sizeof(FaithFrame) == 120, "faith.h layout");
static_assert(sizeof(FaithXform) == 32, "faith.h layout");
#endif

uint32_t faith_api_version(void);
/* The last error on this thread, or "". */
const char* faith_last_error(void);

/* me_install: the Mirror's Edge folder (UTF-8), or NULL for the usual places. Without it the
 * movement works but nothing is animated. */
Faith* faith_create(const char* me_install, float units_per_meter);
void faith_destroy(Faith* f);
uint8_t faith_animated(Faith* f);

/* Replace the collision: `count` triangles, 9 floats each. */
void faith_set_world(Faith* f, const float* tris, uint32_t count);
void faith_teleport(Faith* f, FaithVec3 feet, float heading);
void faith_step(Faith* f, float dt, const FaithInput* input, FaithFrame* out);
const char* faith_state_name(Faith* f);
const char* faith_anim_name(Faith* f);

/* Bind a host skeleton (bones in parent order; parents[i] < i, or -1) with its rest local
 * transforms. kind 0: the whole body (third person); 1: first-person arms. Returns an id or -1. */
int32_t faith_bind_skeleton(Faith* f, uint32_t kind, uint32_t count, const char* const* names, const int32_t* parents,
                            const FaithXform* rest);
uint32_t faith_skeleton_mapped(Faith* f, int32_t skeleton);
/* This frame's local transforms for every bone of a bound skeleton (`count` as bound), given
 * the world transform of the node its root bones hang from. Returns 1 if posed. */
uint8_t faith_pose_skeleton(Faith* f, int32_t skeleton, FaithXform root_parent, FaithXform* out);
/* With flags: FAITH_POSE_PIN_HANDS pins the skeleton's extra anchors too. The first-person arms:
 * the hands exactly on Faith's (what they hold goes in her grip, when her own body is drawn). The
 * body: the shoulders exactly on Faith's against her camera (the body seen in first person). */
#define FAITH_POSE_PIN_HANDS 1u
/* With PIN_HANDS: the arms reach for where Faith's hands appear on screen, for a body drawn with
 * a narrower view than her arms (her arms use Mirror's Edge's 100 degrees): k from
 * faith_set_screen_scale = tan(the body's horizontal half FOV) / tan(her arms'). */
#define FAITH_POSE_SCREEN_MATCH 2u
void faith_set_screen_scale(Faith* f, float k);

/* Mirror's Edge's auto step-up (TdMove_AutoStepUp, shipped switched off in the game): walking into
 * something 35-48 uu (about 0.35-0.5 m) high, she steps up onto it instead of stopping. */
void faith_set_auto_step_up(Faith* f, uint8_t on);
uint8_t faith_pose_skeleton_ex(Faith* f, int32_t skeleton, FaithXform root_parent, uint32_t flags, FaithXform* out);

/* What moves in the host's world (doors, gates, drawbridges), given again every frame: triangles
 * as faith_set_world, collided with together with it. An empty list clears it. */
void faith_set_moving(Faith* f, const float* tris, uint32_t count);

/* What her feet and hands touch, for her step sounds (Mirror's Edge's sets): 0 concrete (and
 * anything the game has no sound for), 1 wood, 2 metal, 3 metal grating, 4 metal ladder,
 * 5 chain-link, 6 metal pipe, 7 airduct, 8 cardboard, 9 water, 10 glass. */
void faith_set_surfaces(Faith* f, uint32_t feet, uint32_t hands);

/* Faith's attacks on the host's actors, decided as Mirror's Edge decides them (its target
 * choice, hit tests, damage and the momentum of the blow). Give who's around before each
 * faith_step; read what landed after it. Host frame and units. */
typedef struct FaithTarget {
    uint32_t id;          /* yours, given back in FaithHit */
    FaithVec3 centre;     /* the middle of its collision capsule */
    float radius, half_height;
    float eye;            /* its eyes above the centre */
    FaithVec3 facing;     /* which way it faces (a takedown from behind or the front) */
} FaithTarget;
typedef struct FaithHit {
    uint32_t target;
    float damage;         /* Mirror's Edge's hit points: punch 33.5, air kick 60-100, slide 60, wallrun 80 */
    FaithVec3 momentum;   /* units/s */
    uint32_t kind;        /* 0 punch, 1 air kick, 2 slide kick, 3 wallrun kick, 4 crouch attack */
} FaithHit;
#ifdef __cplusplus
static_assert(sizeof(FaithTarget) == 40 && sizeof(FaithHit) == 24, "faith.h layout");
#endif
void     faith_set_targets(Faith* f, const FaithTarget* targets, uint32_t count);
uint32_t faith_melee_hits(Faith* f, FaithHit* out, uint32_t max);  /* this step's; out NULL to count */

/* Mirror's Edge's disarm (TdMOVE_Disarm) as a takedown: FaithInput.takedown_pressed with a
 * target in front of her. At the start (done 0) hold the target at enemy_at facing enemy_dir (as
 * it stood: she goes round to where the takedown needs her); pose its side of it
 * (faith_pose_victim) at clip_at facing clip_dir (her spot, the way she faces them); when her clip ends (done 1) it's yours to finish (knock down, kill...).
 * anim: 0-2 a front snatch, 3 from behind. */
typedef struct FaithTakedown {
    uint32_t target;
    uint32_t anim;
    uint32_t done;
    FaithVec3 enemy_at;
    FaithVec3 enemy_dir;
    FaithVec3 clip_at;
    FaithVec3 clip_dir;
} FaithTakedown;
#ifdef __cplusplus
static_assert(sizeof(FaithTakedown) == 60, "faith.h layout");
#endif
uint32_t faith_takedowns(Faith* f, FaithTakedown* out, uint32_t max);  /* this step's; out NULL to count */

/* The takedown's victim side: Mirror's Edge's enemy clip for the same takedown (the patrol
 * cop's), retargeted onto a host skeleton. Bind like faith_bind_skeleton (the whole body); then
 * each frame of the takedown pose it `time` seconds in, feet at `feet` facing `heading` (as
 * FaithFrame.heading), and write the local transforms onto the live nodes. */
int32_t faith_bind_victim(Faith* f, uint32_t count, const char* const* names, const int32_t* parents, const FaithXform* rest);
uint8_t faith_pose_victim(Faith* f, int32_t skeleton, uint32_t anim, float time, FaithVec3 feet, float heading, FaithXform root_parent, FaithXform* out);

/* The ground under a host point: the first solid below it (from 0.5 m above, up to `drop` units
 * down) in the world Faith moves in now (a course's own while on one), so you can stand your
 * people on what only Faith collides with. 0 if there's nothing. */
uint8_t faith_ground_below(Faith* f, FaithVec3 at, float drop, FaithVec3* out);

/* Ziplines, swing poles and balance beams in the host's own world. Before faith_set_world,
 * pass the thin capsules (centre line, radius) and long thin boxes (middle of the top, half the
 * width) in the collision; the ones that work as Mirror's Edge's fixtures are found with it. */
typedef struct FaithFixtureCandidate {
    FaithVec3 a, b;
    float thickness;
    uint32_t capsule;
} FaithFixtureCandidate;
typedef struct FaithFixture {
    uint32_t kind;   /* 0 zipline (a: the high end), 1 swing pole, 2 balance beam, 3 drainpipe (a: its foot, b: the top) */
    FaithVec3 a, b;
} FaithFixture;
#ifdef __cplusplus
static_assert(sizeof(FaithFixtureCandidate) == 32 && sizeof(FaithFixture) == 28, "faith.h layout");
#endif
void     faith_set_fixture_candidates(Faith* f, const FaithFixtureCandidate* c, uint32_t count);
uint32_t faith_world_fixtures(Faith* f, FaithFixture* out, uint32_t max);

/* The app's training maps, played in the host. faith_course_start puts one (0 Moves, 1 Rooftops,
 * 2 Springboard, 3 Training) with its origin at `anchor` (host frame) and makes it Faith's whole
 * world: faith_set_world is kept aside until faith_course_stop (or faith_teleport, which also
 * leaves it). Checkpoints, respawns (falling off, deadly falls) and the time trial run as in the
 * app. The host draws it from faith_course_mesh. */
typedef struct FaithCourseVertex {
    float pos[3], normal[3], uv[2];  /* host frame and units; uv: one grid square a metre */
    uint32_t look;                   /* 0 roof, 1 wall, 2 runner (red), 3 prop, 4 finish, 5 skyline, 6 metal */
} FaithCourseVertex;
typedef struct FaithCourse {
    uint8_t active, running, _pad[2];
    float time, last, best;          /* the time trial; last and best -1 until there's one */
    uint32_t checkpoint, checkpoints;
    uint32_t message_seq;            /* changes with each new faith_course_message */
} FaithCourse;
#ifdef __cplusplus
static_assert(sizeof(FaithCourseVertex) == 36 && sizeof(FaithCourse) == 28, "faith.h layout");
#endif
uint8_t     faith_course_start(Faith* f, uint32_t map, FaithVec3 anchor);  /* 0: no such map */
void        faith_course_stop(Faith* f);
void        faith_course_respawn(Faith* f, int32_t checkpoint);           /* < 0: the current one */
uint8_t     faith_course_status(Faith* f, FaithCourse* out);              /* whether there's a course */
const char* faith_course_message(Faith* f);  /* "Checkpoint - M2 Balance", "Finish 0:41.20 - new best" */
const char* faith_course_name(Faith* f);
const char* faith_course_checkpoint_name(Faith* f, uint32_t i);
uint32_t    faith_course_mesh(Faith* f, FaithCourseVertex* out, uint32_t max);  /* vertices (3 a triangle, grouped by look); max 0 to count */

/* Play one of Faith's idles now (standing still only). */
uint8_t faith_play_idle(Faith* f); /* 0: not now (moving, in the air, crouched) or not animated */

/* Faith's sounds, played on the default sound device from the Mirror's Edge install. */
uint32_t faith_sound_cues(Faith* f);           /* how many loaded (0: none) */
void faith_sound_volume(Faith* f, float gain); /* 0..1, default 0.8 */
void faith_sound_pause(Faith* f, uint8_t paused);

/* ---- Faith's own first-person body (Mirror's Edge's arms and torso, legs), to draw ---- */

/* Camera space (host units; x right, y up, z back towards the viewer); tangent w: bitangent
 * = w * cross(normal, tangent). */
typedef struct FaithVertex {
    float pos[3];
    float normal[3];
    float tangent[4];
    float uv[2];
} FaithVertex;

typedef struct FaithSection {
    uint32_t first_index, index_count, material;
} FaithSection;

typedef struct FaithPartInfo {
    uint32_t vertex_count, index_count, section_count, material_count;
    uint8_t legs;
    uint8_t _pad[3];
} FaithPartInfo;

#ifdef __cplusplus
static_assert(sizeof(FaithVertex) == 48, "faith.h layout");
static_assert(sizeof(FaithSection) == 12, "faith.h layout");
static_assert(sizeof(FaithPartInfo) == 20, "faith.h layout");
#endif

/* How many parts (0 without Mirror's Edge; arms and torso, then legs). */
uint32_t faith_body_parts(Faith* f);
uint8_t faith_body_part(Faith* f, uint32_t part, FaithPartInfo* out);
/* Counter-clockwise front faces in camera space. */
const uint32_t* faith_body_indices(Faith* f, uint32_t part);
const FaithSection* faith_body_sections(Faith* f, uint32_t part);
/* RGBA8: kind 0 colour, 1 normal map, 2 specular. NULL if the material has none. */
const uint8_t* faith_body_texture(Faith* f, uint32_t part, uint32_t material, uint32_t kind, uint32_t* width, uint32_t* height);
const char* faith_body_material_name(Faith* f, uint32_t part, uint32_t material);
/* This frame's pose (after faith_step): vertex_count vertices. Returns 1 if skinned. */
uint8_t faith_body_skin(Faith* f, uint32_t part, FaithVertex* out);

/* The host's own fixtures, with the next faith_set_world (replaces the last ones). kind 0 ladder:
 * a its foot on the wall's face, n out from the wall, top the height she climbs out onto; flags 1
 * drainpipe, 2 she can climb out over the top. 1 door: the closed door's box a..b, n the side she
 * comes from. 2 soft landing: the pad's box a..b. */
typedef struct FaithHostFixture {
    uint32_t kind, flags;
    FaithVec3 a, b, n;
    float top;
} FaithHostFixture;
#ifdef __cplusplus
static_assert(sizeof(FaithHostFixture) == 48, "faith.h layout");
#endif
void faith_set_host_fixtures(Faith* f, const FaithHostFixture* fixtures, uint32_t count);
/* The doors she burst open since the last step (indices into the host fixtures); NULL to count. */
uint32_t faith_doors_opened(Faith* f, uint32_t* out, uint32_t max);
/* The tallest step the auto step-up takes (host units; Mirror's Edge's is 0.48 m). */
void faith_set_auto_step_up_max(Faith* f, float height);
/* One of her bones this frame, in camera space as faith_body_skin's vertices (what the host puts
 * in her hands). 0: no such bone. */
uint8_t faith_body_bone(Faith* f, const char* name, FaithXform* out);

#ifdef __cplusplus
}
#endif
