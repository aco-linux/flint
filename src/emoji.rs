use crate::item::{Action, Icon, Item, Kind};

struct Emoji {
    glyph: &'static str,
    name: &'static str,
    shortcode: &'static str,
    keywords: &'static str,
}

const TABLE: &[Emoji] = &[
    Emoji {
        glyph: "😀",
        name: "Grinning face",
        shortcode: "grinning",
        keywords: "smile happy",
    },
    Emoji {
        glyph: "😃",
        name: "Grinning with big eyes",
        shortcode: "smiley",
        keywords: "smile happy",
    },
    Emoji {
        glyph: "😄",
        name: "Grinning with smiling eyes",
        shortcode: "smile",
        keywords: "happy laugh",
    },
    Emoji {
        glyph: "😁",
        name: "Beaming face",
        shortcode: "grin",
        keywords: "smile happy",
    },
    Emoji {
        glyph: "😆",
        name: "Grinning squinting",
        shortcode: "laughing",
        keywords: "lol haha",
    },
    Emoji {
        glyph: "😅",
        name: "Grinning with sweat",
        shortcode: "sweat_smile",
        keywords: "relief nervous",
    },
    Emoji {
        glyph: "🤣",
        name: "Rolling on the floor laughing",
        shortcode: "rofl",
        keywords: "lol laugh",
    },
    Emoji {
        glyph: "😂",
        name: "Tears of joy",
        shortcode: "joy",
        keywords: "lol laugh cry",
    },
    Emoji {
        glyph: "🙂",
        name: "Slightly smiling",
        shortcode: "slightly_smiling_face",
        keywords: "smile",
    },
    Emoji {
        glyph: "🙃",
        name: "Upside-down face",
        shortcode: "upside_down_face",
        keywords: "sarcasm",
    },
    Emoji {
        glyph: "😉",
        name: "Winking face",
        shortcode: "wink",
        keywords: "flirt",
    },
    Emoji {
        glyph: "😊",
        name: "Smiling with smiling eyes",
        shortcode: "blush",
        keywords: "smile happy shy",
    },
    Emoji {
        glyph: "😇",
        name: "Smiling with halo",
        shortcode: "innocent",
        keywords: "angel",
    },
    Emoji {
        glyph: "🥰",
        name: "Smiling with hearts",
        shortcode: "smiling_face_with_hearts",
        keywords: "love",
    },
    Emoji {
        glyph: "😍",
        name: "Heart eyes",
        shortcode: "heart_eyes",
        keywords: "love crush",
    },
    Emoji {
        glyph: "🤩",
        name: "Star-struck",
        shortcode: "star_struck",
        keywords: "star wow",
    },
    Emoji {
        glyph: "😘",
        name: "Face blowing a kiss",
        shortcode: "kissing_heart",
        keywords: "kiss love",
    },
    Emoji {
        glyph: "😗",
        name: "Kissing face",
        shortcode: "kissing",
        keywords: "kiss",
    },
    Emoji {
        glyph: "☺️",
        name: "Smiling face",
        shortcode: "relaxed",
        keywords: "smile",
    },
    Emoji {
        glyph: "😚",
        name: "Kissing closed eyes",
        shortcode: "kissing_closed_eyes",
        keywords: "kiss",
    },
    Emoji {
        glyph: "😙",
        name: "Kissing smiling eyes",
        shortcode: "kissing_smiling_eyes",
        keywords: "kiss",
    },
    Emoji {
        glyph: "🥲",
        name: "Smiling with tear",
        shortcode: "smiling_face_with_tear",
        keywords: "proud sad",
    },
    Emoji {
        glyph: "😋",
        name: "Savoring food",
        shortcode: "yum",
        keywords: "tasty delicious",
    },
    Emoji {
        glyph: "😛",
        name: "Face with tongue",
        shortcode: "stuck_out_tongue",
        keywords: "tongue",
    },
    Emoji {
        glyph: "😜",
        name: "Winking with tongue",
        shortcode: "stuck_out_tongue_winking_eye",
        keywords: "joke",
    },
    Emoji {
        glyph: "🤪",
        name: "Zany face",
        shortcode: "zany_face",
        keywords: "goofy crazy",
    },
    Emoji {
        glyph: "😝",
        name: "Squinting with tongue",
        shortcode: "stuck_out_tongue_closed_eyes",
        keywords: "joke",
    },
    Emoji {
        glyph: "🤑",
        name: "Money-mouth face",
        shortcode: "money_mouth_face",
        keywords: "rich money",
    },
    Emoji {
        glyph: "🤗",
        name: "Hugging face",
        shortcode: "hugs",
        keywords: "hug",
    },
    Emoji {
        glyph: "🤭",
        name: "Face with hand over mouth",
        shortcode: "hand_over_mouth",
        keywords: "oops giggle",
    },
    Emoji {
        glyph: "🤫",
        name: "Shushing face",
        shortcode: "shushing_face",
        keywords: "quiet secret",
    },
    Emoji {
        glyph: "🤔",
        name: "Thinking face",
        shortcode: "thinking",
        keywords: "hmm think",
    },
    Emoji {
        glyph: "🤐",
        name: "Zipper-mouth face",
        shortcode: "zipper_mouth_face",
        keywords: "quiet secret",
    },
    Emoji {
        glyph: "🤨",
        name: "Raised eyebrow",
        shortcode: "raised_eyebrow",
        keywords: "skeptical",
    },
    Emoji {
        glyph: "😐",
        name: "Neutral face",
        shortcode: "neutral_face",
        keywords: "meh",
    },
    Emoji {
        glyph: "😑",
        name: "Expressionless",
        shortcode: "expressionless",
        keywords: "blank",
    },
    Emoji {
        glyph: "😶",
        name: "Face without mouth",
        shortcode: "no_mouth",
        keywords: "silent",
    },
    Emoji {
        glyph: "😏",
        name: "Smirking face",
        shortcode: "smirk",
        keywords: "smug",
    },
    Emoji {
        glyph: "😒",
        name: "Unamused face",
        shortcode: "unamused",
        keywords: "meh",
    },
    Emoji {
        glyph: "🙄",
        name: "Rolling eyes",
        shortcode: "roll_eyes",
        keywords: "eyeroll",
    },
    Emoji {
        glyph: "😬",
        name: "Grimacing face",
        shortcode: "grimacing",
        keywords: "awkward",
    },
    Emoji {
        glyph: "🤥",
        name: "Lying face",
        shortcode: "lying_face",
        keywords: "lie pinocchio",
    },
    Emoji {
        glyph: "😌",
        name: "Relieved face",
        shortcode: "relieved",
        keywords: "calm",
    },
    Emoji {
        glyph: "😔",
        name: "Pensive face",
        shortcode: "pensive",
        keywords: "sad",
    },
    Emoji {
        glyph: "😪",
        name: "Sleepy face",
        shortcode: "sleepy",
        keywords: "tired",
    },
    Emoji {
        glyph: "🤤",
        name: "Drooling face",
        shortcode: "drooling_face",
        keywords: "hungry",
    },
    Emoji {
        glyph: "😴",
        name: "Sleeping face",
        shortcode: "sleeping",
        keywords: "sleep zzz",
    },
    Emoji {
        glyph: "😷",
        name: "Face with medical mask",
        shortcode: "mask",
        keywords: "sick covid",
    },
    Emoji {
        glyph: "🤒",
        name: "Face with thermometer",
        shortcode: "face_with_thermometer",
        keywords: "sick fever",
    },
    Emoji {
        glyph: "🤕",
        name: "Face with head-bandage",
        shortcode: "face_with_head_bandage",
        keywords: "hurt",
    },
    Emoji {
        glyph: "🤢",
        name: "Nauseated face",
        shortcode: "nauseated_face",
        keywords: "sick vomit",
    },
    Emoji {
        glyph: "🤮",
        name: "Face vomiting",
        shortcode: "vomiting_face",
        keywords: "sick",
    },
    Emoji {
        glyph: "🤧",
        name: "Sneezing face",
        shortcode: "sneezing_face",
        keywords: "sick sneeze",
    },
    Emoji {
        glyph: "🥵",
        name: "Hot face",
        shortcode: "hot_face",
        keywords: "heat",
    },
    Emoji {
        glyph: "🥶",
        name: "Cold face",
        shortcode: "cold_face",
        keywords: "freeze",
    },
    Emoji {
        glyph: "🥴",
        name: "Woozy face",
        shortcode: "woozy_face",
        keywords: "drunk dizzy",
    },
    Emoji {
        glyph: "😵",
        name: "Dizzy face",
        shortcode: "dizzy_face",
        keywords: "dead",
    },
    Emoji {
        glyph: "🤯",
        name: "Exploding head",
        shortcode: "exploding_head",
        keywords: "mind blown",
    },
    Emoji {
        glyph: "🤠",
        name: "Cowboy hat face",
        shortcode: "cowboy_hat_face",
        keywords: "cowboy",
    },
    Emoji {
        glyph: "🥳",
        name: "Partying face",
        shortcode: "partying_face",
        keywords: "party celebrate",
    },
    Emoji {
        glyph: "😎",
        name: "Smiling with sunglasses",
        shortcode: "sunglasses",
        keywords: "cool",
    },
    Emoji {
        glyph: "🤓",
        name: "Nerd face",
        shortcode: "nerd_face",
        keywords: "geek",
    },
    Emoji {
        glyph: "🧐",
        name: "Face with monocle",
        shortcode: "monocle_face",
        keywords: "inspect",
    },
    Emoji {
        glyph: "😕",
        name: "Confused face",
        shortcode: "confused",
        keywords: "huh",
    },
    Emoji {
        glyph: "😟",
        name: "Worried face",
        shortcode: "worried",
        keywords: "sad",
    },
    Emoji {
        glyph: "🙁",
        name: "Slightly frowning",
        shortcode: "slightly_frowning_face",
        keywords: "sad",
    },
    Emoji {
        glyph: "☹️",
        name: "Frowning face",
        shortcode: "frowning_face",
        keywords: "sad",
    },
    Emoji {
        glyph: "😮",
        name: "Face with open mouth",
        shortcode: "open_mouth",
        keywords: "wow surprise",
    },
    Emoji {
        glyph: "😯",
        name: "Hushed face",
        shortcode: "hushed",
        keywords: "wow",
    },
    Emoji {
        glyph: "😲",
        name: "Astonished face",
        shortcode: "astonished",
        keywords: "shock",
    },
    Emoji {
        glyph: "😳",
        name: "Flushed face",
        shortcode: "flushed",
        keywords: "embarrassed",
    },
    Emoji {
        glyph: "🥺",
        name: "Pleading face",
        shortcode: "pleading_face",
        keywords: "please puppy",
    },
    Emoji {
        glyph: "😦",
        name: "Frowning with open mouth",
        shortcode: "frowning",
        keywords: "sad",
    },
    Emoji {
        glyph: "😧",
        name: "Anguished face",
        shortcode: "anguished",
        keywords: "sad",
    },
    Emoji {
        glyph: "😨",
        name: "Fearful face",
        shortcode: "fearful",
        keywords: "scared",
    },
    Emoji {
        glyph: "😰",
        name: "Anxious with sweat",
        shortcode: "cold_sweat",
        keywords: "nervous",
    },
    Emoji {
        glyph: "😥",
        name: "Sad but relieved",
        shortcode: "disappointed_relieved",
        keywords: "sad",
    },
    Emoji {
        glyph: "😢",
        name: "Crying face",
        shortcode: "cry",
        keywords: "sad tear",
    },
    Emoji {
        glyph: "😭",
        name: "Loudly crying",
        shortcode: "sob",
        keywords: "sad cry",
    },
    Emoji {
        glyph: "😱",
        name: "Scream in fear",
        shortcode: "scream",
        keywords: "scared shock",
    },
    Emoji {
        glyph: "😖",
        name: "Confounded face",
        shortcode: "confounded",
        keywords: "upset",
    },
    Emoji {
        glyph: "😣",
        name: "Persevering face",
        shortcode: "persevere",
        keywords: "struggle",
    },
    Emoji {
        glyph: "😞",
        name: "Disappointed face",
        shortcode: "disappointed",
        keywords: "sad",
    },
    Emoji {
        glyph: "😓",
        name: "Downcast with sweat",
        shortcode: "sweat",
        keywords: "sad",
    },
    Emoji {
        glyph: "😩",
        name: "Weary face",
        shortcode: "weary",
        keywords: "tired",
    },
    Emoji {
        glyph: "😫",
        name: "Tired face",
        shortcode: "tired_face",
        keywords: "tired",
    },
    Emoji {
        glyph: "🥱",
        name: "Yawning face",
        shortcode: "yawning_face",
        keywords: "tired bored",
    },
    Emoji {
        glyph: "😤",
        name: "Face with steam",
        shortcode: "triumph",
        keywords: "hmph angry",
    },
    Emoji {
        glyph: "😡",
        name: "Pouting face",
        shortcode: "rage",
        keywords: "angry mad",
    },
    Emoji {
        glyph: "😠",
        name: "Angry face",
        shortcode: "angry",
        keywords: "mad",
    },
    Emoji {
        glyph: "🤬",
        name: "Face with symbols",
        shortcode: "cursing_face",
        keywords: "swear angry",
    },
    Emoji {
        glyph: "😈",
        name: "Smiling horns",
        shortcode: "smiling_imp",
        keywords: "devil",
    },
    Emoji {
        glyph: "👿",
        name: "Angry horns",
        shortcode: "imp",
        keywords: "devil",
    },
    Emoji {
        glyph: "💀",
        name: "Skull",
        shortcode: "skull",
        keywords: "dead",
    },
    Emoji {
        glyph: "☠️",
        name: "Skull and crossbones",
        shortcode: "skull_and_crossbones",
        keywords: "dead poison",
    },
    Emoji {
        glyph: "💩",
        name: "Pile of poo",
        shortcode: "poop",
        keywords: "poo crap",
    },
    Emoji {
        glyph: "🤡",
        name: "Clown face",
        shortcode: "clown_face",
        keywords: "clown",
    },
    Emoji {
        glyph: "👻",
        name: "Ghost",
        shortcode: "ghost",
        keywords: "halloween boo",
    },
    Emoji {
        glyph: "👽",
        name: "Alien",
        shortcode: "alien",
        keywords: "ufo",
    },
    Emoji {
        glyph: "👾",
        name: "Alien monster",
        shortcode: "space_invader",
        keywords: "game",
    },
    Emoji {
        glyph: "🤖",
        name: "Robot",
        shortcode: "robot",
        keywords: "bot",
    },
    Emoji {
        glyph: "😺",
        name: "Grinning cat",
        shortcode: "smiley_cat",
        keywords: "cat smile",
    },
    Emoji {
        glyph: "😸",
        name: "Grinning cat with smiling eyes",
        shortcode: "smile_cat",
        keywords: "cat",
    },
    Emoji {
        glyph: "😹",
        name: "Cat with tears of joy",
        shortcode: "joy_cat",
        keywords: "cat laugh",
    },
    Emoji {
        glyph: "😻",
        name: "Heart-eyes cat",
        shortcode: "heart_eyes_cat",
        keywords: "cat love",
    },
    Emoji {
        glyph: "😼",
        name: "Cat with wry smile",
        shortcode: "smirk_cat",
        keywords: "cat",
    },
    Emoji {
        glyph: "😽",
        name: "Kissing cat",
        shortcode: "kissing_cat",
        keywords: "cat",
    },
    Emoji {
        glyph: "🙀",
        name: "Weary cat",
        shortcode: "scream_cat",
        keywords: "cat shock",
    },
    Emoji {
        glyph: "😿",
        name: "Crying cat",
        shortcode: "crying_cat_face",
        keywords: "cat sad",
    },
    Emoji {
        glyph: "😾",
        name: "Pouting cat",
        shortcode: "pouting_cat",
        keywords: "cat",
    },
    Emoji {
        glyph: "🙈",
        name: "See-no-evil monkey",
        shortcode: "see_no_evil",
        keywords: "monkey",
    },
    Emoji {
        glyph: "🙉",
        name: "Hear-no-evil monkey",
        shortcode: "hear_no_evil",
        keywords: "monkey",
    },
    Emoji {
        glyph: "🙊",
        name: "Speak-no-evil monkey",
        shortcode: "speak_no_evil",
        keywords: "monkey",
    },
    Emoji {
        glyph: "💋",
        name: "Kiss mark",
        shortcode: "kiss",
        keywords: "lips",
    },
    Emoji {
        glyph: "💌",
        name: "Love letter",
        shortcode: "love_letter",
        keywords: "mail heart",
    },
    Emoji {
        glyph: "💘",
        name: "Heart with arrow",
        shortcode: "cupid",
        keywords: "love",
    },
    Emoji {
        glyph: "💝",
        name: "Heart with ribbon",
        shortcode: "gift_heart",
        keywords: "love",
    },
    Emoji {
        glyph: "💖",
        name: "Sparkling heart",
        shortcode: "sparkling_heart",
        keywords: "love",
    },
    Emoji {
        glyph: "💗",
        name: "Growing heart",
        shortcode: "heartpulse",
        keywords: "love",
    },
    Emoji {
        glyph: "💓",
        name: "Beating heart",
        shortcode: "heartbeat",
        keywords: "love",
    },
    Emoji {
        glyph: "💞",
        name: "Revolving hearts",
        shortcode: "revolving_hearts",
        keywords: "love",
    },
    Emoji {
        glyph: "💕",
        name: "Two hearts",
        shortcode: "two_hearts",
        keywords: "love",
    },
    Emoji {
        glyph: "💟",
        name: "Heart decoration",
        shortcode: "heart_decoration",
        keywords: "love",
    },
    Emoji {
        glyph: "❣️",
        name: "Heart exclamation",
        shortcode: "heavy_heart_exclamation",
        keywords: "love",
    },
    Emoji {
        glyph: "💔",
        name: "Broken heart",
        shortcode: "broken_heart",
        keywords: "sad love",
    },
    Emoji {
        glyph: "❤️",
        name: "Red heart",
        shortcode: "heart",
        keywords: "love red",
    },
    Emoji {
        glyph: "🧡",
        name: "Orange heart",
        shortcode: "orange_heart",
        keywords: "love",
    },
    Emoji {
        glyph: "💛",
        name: "Yellow heart",
        shortcode: "yellow_heart",
        keywords: "love",
    },
    Emoji {
        glyph: "💚",
        name: "Green heart",
        shortcode: "green_heart",
        keywords: "love",
    },
    Emoji {
        glyph: "💙",
        name: "Blue heart",
        shortcode: "blue_heart",
        keywords: "love",
    },
    Emoji {
        glyph: "💜",
        name: "Purple heart",
        shortcode: "purple_heart",
        keywords: "love",
    },
    Emoji {
        glyph: "🖤",
        name: "Black heart",
        shortcode: "black_heart",
        keywords: "love",
    },
    Emoji {
        glyph: "🤍",
        name: "White heart",
        shortcode: "white_heart",
        keywords: "love",
    },
    Emoji {
        glyph: "🤎",
        name: "Brown heart",
        shortcode: "brown_heart",
        keywords: "love",
    },
    Emoji {
        glyph: "💯",
        name: "Hundred points",
        shortcode: "100",
        keywords: "score perfect",
    },
    Emoji {
        glyph: "💢",
        name: "Anger symbol",
        shortcode: "anger",
        keywords: "mad",
    },
    Emoji {
        glyph: "💥",
        name: "Collision",
        shortcode: "boom",
        keywords: "explode",
    },
    Emoji {
        glyph: "💫",
        name: "Dizzy",
        shortcode: "dizzy",
        keywords: "star",
    },
    Emoji {
        glyph: "💦",
        name: "Sweat droplets",
        shortcode: "sweat_drops",
        keywords: "water",
    },
    Emoji {
        glyph: "💨",
        name: "Dashing away",
        shortcode: "dash",
        keywords: "fast",
    },
    Emoji {
        glyph: "👋",
        name: "Waving hand",
        shortcode: "wave",
        keywords: "hello hi bye",
    },
    Emoji {
        glyph: "🤚",
        name: "Raised back of hand",
        shortcode: "raised_back_of_hand",
        keywords: "hand",
    },
    Emoji {
        glyph: "🖐️",
        name: "Hand with fingers splayed",
        shortcode: "hand_splayed",
        keywords: "hand",
    },
    Emoji {
        glyph: "✋",
        name: "Raised hand",
        shortcode: "hand",
        keywords: "stop highfive",
    },
    Emoji {
        glyph: "🖖",
        name: "Vulcan salute",
        shortcode: "vulcan_salute",
        keywords: "spock",
    },
    Emoji {
        glyph: "👌",
        name: "OK hand",
        shortcode: "ok_hand",
        keywords: "ok okay",
    },
    Emoji {
        glyph: "🤌",
        name: "Pinched fingers",
        shortcode: "pinched_fingers",
        keywords: "italian",
    },
    Emoji {
        glyph: "🤏",
        name: "Pinching hand",
        shortcode: "pinching_hand",
        keywords: "small",
    },
    Emoji {
        glyph: "✌️",
        name: "Victory hand",
        shortcode: "v",
        keywords: "peace",
    },
    Emoji {
        glyph: "🤞",
        name: "Crossed fingers",
        shortcode: "crossed_fingers",
        keywords: "luck",
    },
    Emoji {
        glyph: "🤟",
        name: "Love-you gesture",
        shortcode: "love_you_gesture",
        keywords: "rock",
    },
    Emoji {
        glyph: "🤘",
        name: "Sign of the horns",
        shortcode: "metal",
        keywords: "rock",
    },
    Emoji {
        glyph: "🤙",
        name: "Call me hand",
        shortcode: "call_me_hand",
        keywords: "phone",
    },
    Emoji {
        glyph: "👈",
        name: "Backhand left",
        shortcode: "point_left",
        keywords: "left",
    },
    Emoji {
        glyph: "👉",
        name: "Backhand right",
        shortcode: "point_right",
        keywords: "right",
    },
    Emoji {
        glyph: "👆",
        name: "Backhand up",
        shortcode: "point_up_2",
        keywords: "up",
    },
    Emoji {
        glyph: "👇",
        name: "Backhand down",
        shortcode: "point_down",
        keywords: "down",
    },
    Emoji {
        glyph: "☝️",
        name: "Index pointing up",
        shortcode: "point_up",
        keywords: "up",
    },
    Emoji {
        glyph: "👍",
        name: "Thumbs up",
        shortcode: "thumbsup",
        keywords: "+1 yes like",
    },
    Emoji {
        glyph: "👎",
        name: "Thumbs down",
        shortcode: "thumbsdown",
        keywords: "-1 no dislike",
    },
    Emoji {
        glyph: "✊",
        name: "Raised fist",
        shortcode: "fist",
        keywords: "power",
    },
    Emoji {
        glyph: "👊",
        name: "Oncoming fist",
        shortcode: "punch",
        keywords: "fist bump",
    },
    Emoji {
        glyph: "🤛",
        name: "Left-facing fist",
        shortcode: "fist_left",
        keywords: "bump",
    },
    Emoji {
        glyph: "🤜",
        name: "Right-facing fist",
        shortcode: "fist_right",
        keywords: "bump",
    },
    Emoji {
        glyph: "👏",
        name: "Clapping hands",
        shortcode: "clap",
        keywords: "applause",
    },
    Emoji {
        glyph: "🙌",
        name: "Raising hands",
        shortcode: "raised_hands",
        keywords: "hooray",
    },
    Emoji {
        glyph: "👐",
        name: "Open hands",
        shortcode: "open_hands",
        keywords: "hug",
    },
    Emoji {
        glyph: "🤲",
        name: "Palms up together",
        shortcode: "palms_up_together",
        keywords: "pray",
    },
    Emoji {
        glyph: "🤝",
        name: "Handshake",
        shortcode: "handshake",
        keywords: "deal",
    },
    Emoji {
        glyph: "🙏",
        name: "Folded hands",
        shortcode: "pray",
        keywords: "please thanks",
    },
    Emoji {
        glyph: "✍️",
        name: "Writing hand",
        shortcode: "writing_hand",
        keywords: "write",
    },
    Emoji {
        glyph: "💅",
        name: "Nail polish",
        shortcode: "nail_care",
        keywords: "nails",
    },
    Emoji {
        glyph: "🤳",
        name: "Selfie",
        shortcode: "selfie",
        keywords: "phone",
    },
    Emoji {
        glyph: "💪",
        name: "Flexed biceps",
        shortcode: "muscle",
        keywords: "strong gym",
    },
    Emoji {
        glyph: "🦾",
        name: "Mechanical arm",
        shortcode: "mechanical_arm",
        keywords: "robot",
    },
    Emoji {
        glyph: "🦵",
        name: "Leg",
        shortcode: "leg",
        keywords: "kick",
    },
    Emoji {
        glyph: "🦶",
        name: "Foot",
        shortcode: "foot",
        keywords: "kick",
    },
    Emoji {
        glyph: "👂",
        name: "Ear",
        shortcode: "ear",
        keywords: "hear",
    },
    Emoji {
        glyph: "👃",
        name: "Nose",
        shortcode: "nose",
        keywords: "smell",
    },
    Emoji {
        glyph: "🧠",
        name: "Brain",
        shortcode: "brain",
        keywords: "smart",
    },
    Emoji {
        glyph: "👀",
        name: "Eyes",
        shortcode: "eyes",
        keywords: "look see",
    },
    Emoji {
        glyph: "👁️",
        name: "Eye",
        shortcode: "eye",
        keywords: "see",
    },
    Emoji {
        glyph: "👅",
        name: "Tongue",
        shortcode: "tongue",
        keywords: "taste",
    },
    Emoji {
        glyph: "👄",
        name: "Mouth",
        shortcode: "lips",
        keywords: "kiss",
    },
    Emoji {
        glyph: "👶",
        name: "Baby",
        shortcode: "baby",
        keywords: "child",
    },
    Emoji {
        glyph: "🧒",
        name: "Child",
        shortcode: "child",
        keywords: "kid",
    },
    Emoji {
        glyph: "👦",
        name: "Boy",
        shortcode: "boy",
        keywords: "kid",
    },
    Emoji {
        glyph: "👧",
        name: "Girl",
        shortcode: "girl",
        keywords: "kid",
    },
    Emoji {
        glyph: "🧑",
        name: "Person",
        shortcode: "adult",
        keywords: "human",
    },
    Emoji {
        glyph: "👨",
        name: "Man",
        shortcode: "man",
        keywords: "male",
    },
    Emoji {
        glyph: "👩",
        name: "Woman",
        shortcode: "woman",
        keywords: "female",
    },
    Emoji {
        glyph: "🧓",
        name: "Older person",
        shortcode: "older_adult",
        keywords: "elder",
    },
    Emoji {
        glyph: "👴",
        name: "Old man",
        shortcode: "older_man",
        keywords: "elder",
    },
    Emoji {
        glyph: "👵",
        name: "Old woman",
        shortcode: "older_woman",
        keywords: "elder",
    },
    Emoji {
        glyph: "🐶",
        name: "Dog face",
        shortcode: "dog",
        keywords: "puppy pet",
    },
    Emoji {
        glyph: "🐱",
        name: "Cat face",
        shortcode: "cat",
        keywords: "kitten pet",
    },
    Emoji {
        glyph: "🐭",
        name: "Mouse face",
        shortcode: "mouse",
        keywords: "rodent",
    },
    Emoji {
        glyph: "🐹",
        name: "Hamster",
        shortcode: "hamster",
        keywords: "pet",
    },
    Emoji {
        glyph: "🐰",
        name: "Rabbit face",
        shortcode: "rabbit",
        keywords: "bunny",
    },
    Emoji {
        glyph: "🦊",
        name: "Fox",
        shortcode: "fox_face",
        keywords: "fox",
    },
    Emoji {
        glyph: "🐻",
        name: "Bear",
        shortcode: "bear",
        keywords: "animal",
    },
    Emoji {
        glyph: "🐼",
        name: "Panda",
        shortcode: "panda_face",
        keywords: "bear",
    },
    Emoji {
        glyph: "🐨",
        name: "Koala",
        shortcode: "koala",
        keywords: "australia",
    },
    Emoji {
        glyph: "🐯",
        name: "Tiger face",
        shortcode: "tiger",
        keywords: "cat",
    },
    Emoji {
        glyph: "🦁",
        name: "Lion",
        shortcode: "lion_face",
        keywords: "cat",
    },
    Emoji {
        glyph: "🐮",
        name: "Cow face",
        shortcode: "cow",
        keywords: "moo",
    },
    Emoji {
        glyph: "🐷",
        name: "Pig face",
        shortcode: "pig",
        keywords: "oink",
    },
    Emoji {
        glyph: "🐸",
        name: "Frog",
        shortcode: "frog",
        keywords: "toad",
    },
    Emoji {
        glyph: "🐵",
        name: "Monkey face",
        shortcode: "monkey_face",
        keywords: "ape",
    },
    Emoji {
        glyph: "🐔",
        name: "Chicken",
        shortcode: "chicken",
        keywords: "bird",
    },
    Emoji {
        glyph: "🐧",
        name: "Penguin",
        shortcode: "penguin",
        keywords: "bird",
    },
    Emoji {
        glyph: "🐦",
        name: "Bird",
        shortcode: "bird",
        keywords: "fly",
    },
    Emoji {
        glyph: "🐤",
        name: "Baby chick",
        shortcode: "baby_chick",
        keywords: "bird",
    },
    Emoji {
        glyph: "🦆",
        name: "Duck",
        shortcode: "duck",
        keywords: "bird",
    },
    Emoji {
        glyph: "🦅",
        name: "Eagle",
        shortcode: "eagle",
        keywords: "bird",
    },
    Emoji {
        glyph: "🦉",
        name: "Owl",
        shortcode: "owl",
        keywords: "bird wise",
    },
    Emoji {
        glyph: "🐝",
        name: "Honeybee",
        shortcode: "bee",
        keywords: "honey",
    },
    Emoji {
        glyph: "🐛",
        name: "Bug",
        shortcode: "bug",
        keywords: "insect",
    },
    Emoji {
        glyph: "🦋",
        name: "Butterfly",
        shortcode: "butterfly",
        keywords: "insect",
    },
    Emoji {
        glyph: "🐢",
        name: "Turtle",
        shortcode: "turtle",
        keywords: "slow",
    },
    Emoji {
        glyph: "🐍",
        name: "Snake",
        shortcode: "snake",
        keywords: "python",
    },
    Emoji {
        glyph: "🐙",
        name: "Octopus",
        shortcode: "octopus",
        keywords: "sea",
    },
    Emoji {
        glyph: "🐠",
        name: "Tropical fish",
        shortcode: "tropical_fish",
        keywords: "sea",
    },
    Emoji {
        glyph: "🐟",
        name: "Fish",
        shortcode: "fish",
        keywords: "sea",
    },
    Emoji {
        glyph: "🐬",
        name: "Dolphin",
        shortcode: "dolphin",
        keywords: "sea",
    },
    Emoji {
        glyph: "🐳",
        name: "Spouting whale",
        shortcode: "whale",
        keywords: "sea",
    },
    Emoji {
        glyph: "🌹",
        name: "Rose",
        shortcode: "rose",
        keywords: "flower love",
    },
    Emoji {
        glyph: "🌷",
        name: "Tulip",
        shortcode: "tulip",
        keywords: "flower",
    },
    Emoji {
        glyph: "🌸",
        name: "Cherry blossom",
        shortcode: "cherry_blossom",
        keywords: "flower spring",
    },
    Emoji {
        glyph: "🌻",
        name: "Sunflower",
        shortcode: "sunflower",
        keywords: "flower",
    },
    Emoji {
        glyph: "🌼",
        name: "Blossom",
        shortcode: "blossom",
        keywords: "flower",
    },
    Emoji {
        glyph: "🌱",
        name: "Seedling",
        shortcode: "seedling",
        keywords: "plant grow",
    },
    Emoji {
        glyph: "🌲",
        name: "Evergreen tree",
        shortcode: "evergreen_tree",
        keywords: "tree",
    },
    Emoji {
        glyph: "🌳",
        name: "Deciduous tree",
        shortcode: "deciduous_tree",
        keywords: "tree",
    },
    Emoji {
        glyph: "🌴",
        name: "Palm tree",
        shortcode: "palm_tree",
        keywords: "tropical",
    },
    Emoji {
        glyph: "🌵",
        name: "Cactus",
        shortcode: "cactus",
        keywords: "desert",
    },
    Emoji {
        glyph: "🍀",
        name: "Four leaf clover",
        shortcode: "four_leaf_clover",
        keywords: "luck",
    },
    Emoji {
        glyph: "🍁",
        name: "Maple leaf",
        shortcode: "maple_leaf",
        keywords: "fall canada",
    },
    Emoji {
        glyph: "🌙",
        name: "Crescent moon",
        shortcode: "crescent_moon",
        keywords: "night",
    },
    Emoji {
        glyph: "⭐",
        name: "Star",
        shortcode: "star",
        keywords: "night",
    },
    Emoji {
        glyph: "🌟",
        name: "Glowing star",
        shortcode: "star2",
        keywords: "shine",
    },
    Emoji {
        glyph: "☀️",
        name: "Sun",
        shortcode: "sunny",
        keywords: "weather hot",
    },
    Emoji {
        glyph: "⛅",
        name: "Sun behind cloud",
        shortcode: "partly_sunny",
        keywords: "weather",
    },
    Emoji {
        glyph: "☁️",
        name: "Cloud",
        shortcode: "cloud",
        keywords: "weather",
    },
    Emoji {
        glyph: "🌧️",
        name: "Cloud with rain",
        shortcode: "cloud_with_rain",
        keywords: "weather rain",
    },
    Emoji {
        glyph: "⛈️",
        name: "Cloud with lightning and rain",
        shortcode: "thunder_cloud_and_rain",
        keywords: "storm",
    },
    Emoji {
        glyph: "❄️",
        name: "Snowflake",
        shortcode: "snowflake",
        keywords: "cold winter",
    },
    Emoji {
        glyph: "⚡",
        name: "High voltage",
        shortcode: "zap",
        keywords: "lightning",
    },
    Emoji {
        glyph: "🔥",
        name: "Fire",
        shortcode: "fire",
        keywords: "hot lit",
    },
    Emoji {
        glyph: "💧",
        name: "Droplet",
        shortcode: "droplet",
        keywords: "water",
    },
    Emoji {
        glyph: "🌊",
        name: "Water wave",
        shortcode: "ocean",
        keywords: "sea",
    },
    Emoji {
        glyph: "🍎",
        name: "Red apple",
        shortcode: "apple",
        keywords: "fruit",
    },
    Emoji {
        glyph: "🍊",
        name: "Tangerine",
        shortcode: "tangerine",
        keywords: "orange fruit",
    },
    Emoji {
        glyph: "🍋",
        name: "Lemon",
        shortcode: "lemon",
        keywords: "fruit",
    },
    Emoji {
        glyph: "🍌",
        name: "Banana",
        shortcode: "banana",
        keywords: "fruit",
    },
    Emoji {
        glyph: "🍉",
        name: "Watermelon",
        shortcode: "watermelon",
        keywords: "fruit",
    },
    Emoji {
        glyph: "🍇",
        name: "Grapes",
        shortcode: "grapes",
        keywords: "fruit",
    },
    Emoji {
        glyph: "🍓",
        name: "Strawberry",
        shortcode: "strawberry",
        keywords: "fruit",
    },
    Emoji {
        glyph: "🍑",
        name: "Peach",
        shortcode: "peach",
        keywords: "fruit",
    },
    Emoji {
        glyph: "🍕",
        name: "Pizza",
        shortcode: "pizza",
        keywords: "food",
    },
    Emoji {
        glyph: "🍔",
        name: "Hamburger",
        shortcode: "hamburger",
        keywords: "food burger",
    },
    Emoji {
        glyph: "🍟",
        name: "French fries",
        shortcode: "fries",
        keywords: "food",
    },
    Emoji {
        glyph: "🌮",
        name: "Taco",
        shortcode: "taco",
        keywords: "food",
    },
    Emoji {
        glyph: "🍣",
        name: "Sushi",
        shortcode: "sushi",
        keywords: "food japan",
    },
    Emoji {
        glyph: "☕",
        name: "Hot beverage",
        shortcode: "coffee",
        keywords: "tea cafe",
    },
    Emoji {
        glyph: "🍺",
        name: "Beer mug",
        shortcode: "beer",
        keywords: "drink alcohol",
    },
    Emoji {
        glyph: "🍻",
        name: "Clinking beer mugs",
        shortcode: "beers",
        keywords: "cheers",
    },
    Emoji {
        glyph: "🍷",
        name: "Wine glass",
        shortcode: "wine_glass",
        keywords: "drink",
    },
    Emoji {
        glyph: "⚽",
        name: "Soccer ball",
        shortcode: "soccer",
        keywords: "sport football",
    },
    Emoji {
        glyph: "🏀",
        name: "Basketball",
        shortcode: "basketball",
        keywords: "sport",
    },
    Emoji {
        glyph: "🏈",
        name: "American football",
        shortcode: "football",
        keywords: "sport",
    },
    Emoji {
        glyph: "🎾",
        name: "Tennis",
        shortcode: "tennis",
        keywords: "sport",
    },
    Emoji {
        glyph: "🎮",
        name: "Video game",
        shortcode: "video_game",
        keywords: "controller play",
    },
    Emoji {
        glyph: "🎵",
        name: "Musical note",
        shortcode: "musical_note",
        keywords: "music",
    },
    Emoji {
        glyph: "🎶",
        name: "Musical notes",
        shortcode: "notes",
        keywords: "music",
    },
    Emoji {
        glyph: "🏆",
        name: "Trophy",
        shortcode: "trophy",
        keywords: "win award",
    },
    Emoji {
        glyph: "🥇",
        name: "Gold medal",
        shortcode: "1st_place_medal",
        keywords: "win",
    },
    Emoji {
        glyph: "🎯",
        name: "Bullseye",
        shortcode: "dart",
        keywords: "target",
    },
    Emoji {
        glyph: "📱",
        name: "Mobile phone",
        shortcode: "iphone",
        keywords: "phone",
    },
    Emoji {
        glyph: "💻",
        name: "Laptop",
        shortcode: "computer",
        keywords: "pc",
    },
    Emoji {
        glyph: "⌨️",
        name: "Keyboard",
        shortcode: "keyboard",
        keywords: "type",
    },
    Emoji {
        glyph: "📷",
        name: "Camera",
        shortcode: "camera",
        keywords: "photo",
    },
    Emoji {
        glyph: "💡",
        name: "Light bulb",
        shortcode: "bulb",
        keywords: "idea",
    },
    Emoji {
        glyph: "🔒",
        name: "Locked",
        shortcode: "lock",
        keywords: "secure",
    },
    Emoji {
        glyph: "🔓",
        name: "Unlocked",
        shortcode: "unlock",
        keywords: "open",
    },
    Emoji {
        glyph: "🔑",
        name: "Key",
        shortcode: "key",
        keywords: "password",
    },
    Emoji {
        glyph: "📧",
        name: "E-mail",
        shortcode: "e-mail",
        keywords: "mail",
    },
    Emoji {
        glyph: "📎",
        name: "Paperclip",
        shortcode: "paperclip",
        keywords: "attach",
    },
    Emoji {
        glyph: "✅",
        name: "Check mark button",
        shortcode: "white_check_mark",
        keywords: "done yes",
    },
    Emoji {
        glyph: "❌",
        name: "Cross mark",
        shortcode: "x",
        keywords: "no wrong",
    },
    Emoji {
        glyph: "❓",
        name: "Question mark",
        shortcode: "question",
        keywords: "ask",
    },
    Emoji {
        glyph: "❗",
        name: "Exclamation mark",
        shortcode: "exclamation",
        keywords: "alert",
    },
    Emoji {
        glyph: "⚠️",
        name: "Warning",
        shortcode: "warning",
        keywords: "alert caution",
    },
    Emoji {
        glyph: "🚀",
        name: "Rocket",
        shortcode: "rocket",
        keywords: "launch ship",
    },
    Emoji {
        glyph: "✈️",
        name: "Airplane",
        shortcode: "airplane",
        keywords: "flight travel",
    },
    Emoji {
        glyph: "🚗",
        name: "Car",
        shortcode: "car",
        keywords: "drive",
    },
    Emoji {
        glyph: "🚕",
        name: "Taxi",
        shortcode: "taxi",
        keywords: "cab",
    },
    Emoji {
        glyph: "🏠",
        name: "House",
        shortcode: "house",
        keywords: "home",
    },
    Emoji {
        glyph: "🎉",
        name: "Party popper",
        shortcode: "tada",
        keywords: "celebrate confetti",
    },
    Emoji {
        glyph: "🎊",
        name: "Confetti ball",
        shortcode: "confetti_ball",
        keywords: "party",
    },
    Emoji {
        glyph: "🎁",
        name: "Wrapped gift",
        shortcode: "gift",
        keywords: "present",
    },
    Emoji {
        glyph: "🎂",
        name: "Birthday cake",
        shortcode: "birthday",
        keywords: "party",
    },
    Emoji {
        glyph: "🎄",
        name: "Christmas tree",
        shortcode: "christmas_tree",
        keywords: "holiday",
    },
    Emoji {
        glyph: "🎃",
        name: "Jack-o-lantern",
        shortcode: "jack_o_lantern",
        keywords: "halloween",
    },
    Emoji {
        glyph: "✨",
        name: "Sparkles",
        shortcode: "sparkles",
        keywords: "shine magic",
    },
    Emoji {
        glyph: "🌈",
        name: "Rainbow",
        shortcode: "rainbow",
        keywords: "pride weather",
    },
    Emoji {
        glyph: "☀️",
        name: "Sun",
        shortcode: "sun",
        keywords: "weather",
    },
];

pub fn search(query: &str, limit: usize) -> Vec<Item> {
    let q = normalize(query);
    if q.is_empty() {
        return TABLE.iter().take(limit).map(to_item).collect();
    }
    let mut ranked: Vec<(u32, &Emoji)> = Vec::new();
    for emoji in TABLE {
        if let Some(score) = rank(emoji, &q, query.trim()) {
            ranked.push((score, emoji));
        }
    }
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.shortcode.cmp(b.1.shortcode)));
    ranked.dedup_by(|a, b| a.1.shortcode == b.1.shortcode);
    ranked.truncate(limit);
    ranked
        .into_iter()
        .map(|(_, emoji)| to_item(emoji))
        .collect()
}

pub fn looks_like_query(query: &str) -> bool {
    let q = query.trim();
    if q.starts_with(':') && q.len() >= 3 {
        return true;
    }
    if q.chars().any(|c| {
        let n = c as u32;
        n >= 0x1F300 || (0x2600..=0x27BF).contains(&n)
    }) {
        return true;
    }
    let key = normalize(q);
    if key.chars().count() < 3 {
        return false;
    }
    TABLE.iter().any(|emoji| matches_key(emoji, &key))
}

fn rank(emoji: &Emoji, key: &str, raw: &str) -> Option<u32> {
    if raw == emoji.glyph {
        return Some(10_000);
    }
    if key == emoji.shortcode {
        return Some(9_000);
    }
    if emoji.name.to_ascii_lowercase() == key {
        return Some(8_000);
    }
    if emoji
        .keywords
        .split_whitespace()
        .chain(emoji.name.split_whitespace())
        .any(|word| word.eq_ignore_ascii_case(key))
    {
        return Some(7_000);
    }
    if emoji.shortcode.starts_with(key) {
        return Some(5_000);
    }
    if matches_key(emoji, key) {
        return Some(3_000);
    }
    None
}

fn matches_key(emoji: &Emoji, key: &str) -> bool {
    if key.is_empty() {
        return false;
    }
    emoji.shortcode.contains(key)
        || emoji.name.to_ascii_lowercase().contains(key)
        || emoji
            .keywords
            .split_whitespace()
            .any(|word| word.starts_with(key) || word == key)
}

fn normalize(query: &str) -> String {
    query
        .trim()
        .trim_matches(':')
        .to_ascii_lowercase()
        .replace(['-', ' '], "_")
}

fn to_item(emoji: &Emoji) -> Item {
    Item {
        id: format!("emoji:{}", emoji.shortcode),
        title: format!("{}  {}", emoji.glyph, emoji.name),
        subtitle: format!(":{}: · paste", emoji.shortcode),
        keywords: format!("{} {} emoji", emoji.shortcode, emoji.keywords),
        kind: Kind::Command,
        icon: Icon::None,
        action: Action::Paste(emoji.glyph.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::{TABLE, looks_like_query, search};
    use crate::item::Action;

    #[test]
    fn table_is_about_two_hundred() {
        assert!(
            TABLE.len() >= 180,
            "expected ~200 emoji, got {}",
            TABLE.len()
        );
    }

    #[test]
    fn smile_and_shortcode_match() {
        let smile = search("smile", 8);
        assert!(
            smile
                .iter()
                .any(|item| item.title.contains("😄") || item.id.contains("smile")),
            "smile should hit the grin/smile rows: {:?}",
            smile.iter().map(|i| &i.id).collect::<Vec<_>>()
        );
        let short = search(":smile:", 4);
        assert!(short.iter().any(|item| item.id == "emoji:smile"));
        assert!(
            matches!(short[0].action, Action::Paste(ref g) if g.contains('😄') || g.contains('😊') || !g.is_empty())
        );
        assert!(looks_like_query("smile"));
        assert!(looks_like_query(":smile:"));
        assert!(!looks_like_query("firefox"));
        assert!(!looks_like_query("sm"));
    }

    #[test]
    fn empty_query_lists_the_table() {
        assert!(!search("", 10).is_empty());
    }
}
