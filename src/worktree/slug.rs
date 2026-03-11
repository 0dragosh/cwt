use rand::seq::SliceRandom;
use rand::Rng;

const ADJECTIVES: &[&str] = &[
    "bold", "calm", "dark", "deft", "fair", "fast", "firm", "glad", "gold", "keen", "kind", "lean",
    "loud", "neat", "pale", "pure", "rare", "rich", "safe", "slim", "soft", "tall", "tidy", "warm",
    "wild", "wise", "blue", "cool", "deep", "dry", "epic", "fine", "free", "gray", "hale", "icy",
    "just", "lush", "mild", "nice",
];

const NOUNS: &[&str] = &[
    "arch", "bark", "beam", "bird", "bolt", "cave", "claw", "cove", "dawn", "dove", "dune", "echo",
    "edge", "elm", "fern", "fire", "flux", "gale", "glen", "glow", "haze", "hill", "isle", "jade",
    "lake", "leaf", "lynx", "mist", "moon", "moss", "nest", "node", "oak", "opal", "palm", "peak",
    "pine", "pond", "rain", "reed", "reef", "rock", "rose", "sage", "sand", "star", "stem", "tide",
    "vale", "vine", "wave", "well", "wind", "wolf", "wren", "yard",
];

/// Generate a random slug like "bold-oak-a3f2".
pub fn generate_slug() -> String {
    let mut rng = rand::thread_rng();
    let adj = ADJECTIVES.choose(&mut rng).unwrap();
    let noun = NOUNS.choose(&mut rng).unwrap();
    let hex: u16 = rng.gen();
    format!("{}-{}-{:04x}", adj, noun, hex)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slug_format() {
        let slug = generate_slug();
        let parts: Vec<&str> = slug.split('-').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[2].len(), 4); // hex4
    }
}
