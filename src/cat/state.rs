// cat/state.rs — État de la machine à états du chat.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CatState {
    Idle,
    Walking,
    Eating,
    Drinking,
    Angry,
    Sleeping,
    WakingUp,
}

impl CatState {
    /// Retourne la clé d'animation correspondante dans metadata.json.
    /// `None` pour Idle/Sleeping (frames de rotation statiques).
    pub fn anim_key(self) -> Option<&'static str> {
        match self {
            CatState::Walking => Some("running-8-frames"),
            CatState::Eating => Some("eating"),
            CatState::Drinking => Some("drinking"),
            CatState::Angry => Some("angry"),
            CatState::WakingUp => Some("waking-getting-up"),
            CatState::Idle | CatState::Sleeping => None,
        }
    }

    /// Les états one-shot retournent à Idle une fois l'animation terminée.
    pub fn is_one_shot(self) -> bool {
        matches!(
            self,
            CatState::Eating | CatState::Drinking | CatState::Angry | CatState::WakingUp
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    South,
    SouthEast,
    East,
    NorthEast,
    North,
    NorthWest,
    West,
    SouthWest,
}

impl Direction {
    pub fn as_str(self) -> &'static str {
        match self {
            Direction::South => "south",
            Direction::SouthEast => "south-east",
            Direction::East => "east",
            Direction::NorthEast => "north-east",
            Direction::North => "north",
            Direction::NorthWest => "north-west",
            Direction::West => "west",
            Direction::SouthWest => "south-west",
        }
    }
}
