//! The Président, the card game: its rules, apart from what draws it (see `app::president`), so that
//! floor can follow the same ones. 52 cards from the 3 (the weakest) to the 2 (the strongest); in turn,
//! each one plays as many cards of one value as the trick asks, stronger, or passes (and is out of the
//! trick). Once all the others passed, the last one who played leads again; a 2 closes the trick at once.
//! The order out of cards gives the roles of the next round (Président, Vice-président, Neutre,
//! Vice-trou, Trou) and the cards given: the Trou his two best to the Président, who gives back two of
//! his choice; the Vice-trou his best one to the Vice-président, who gives one back.

use crate::connect4::Level;

/// The values, weakest first: 3 4 5 6 7 8 9 10 J Q K A 2.
pub const RANKS: u8 = 13;
/// The 2's value, the strongest: it closes the trick.
pub const TWO: u8 = 12;

/// A card: its value (0, the 3, to 12, the 2), its suit (0 ♣, 1 ♦, 2 ♥, 3 ♠).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Card {
    pub rank: u8,
    pub suit: u8,
}

impl Card {
    /// The 3 of clubs: who holds it leads the first round.
    pub const FIRST: Card = Card { rank: 0, suit: 0 };

    /// Its value as written on it.
    pub fn label(self) -> &'static str {
        ["3", "4", "5", "6", "7", "8", "9", "10", "V", "D", "R", "A", "2"][self.rank as usize]
    }

    pub fn red(self) -> bool {
        matches!(self.suit, 1 | 2)
    }
}

/// A role, from a round's order out of cards.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    President,
    Vice,
    Neutral,
    ViceTrou,
    Trou,
}

/// The role of the one out in `place` (0: first) of `n` players: no Vices at 3.
pub fn role(place: usize, n: usize) -> Role {
    let vices = n >= 4;
    match place {
        0 => Role::President,
        p if p + 1 == n => Role::Trou,
        1 if vices => Role::Vice,
        p if vices && p + 2 == n => Role::ViceTrou,
        _ => Role::Neutral,
    }
}

/// A small random generator (xorshift), for the deal and the computer.
pub struct Dice(u64);

impl Dice {
    pub fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    pub fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n.max(1) as u64) as usize
    }
}

/// The 52 cards shuffled, dealt one each in turn from `first`: the hands sorted, weakest first.
pub fn deal(n: usize, first: usize, dice: &mut Dice) -> Vec<Vec<Card>> {
    let mut deck: Vec<Card> = (0..RANKS).flat_map(|rank| (0..4).map(move |suit| Card { rank, suit })).collect();
    for i in (1..deck.len()).rev() {
        deck.swap(i, dice.below(i + 1));
    }
    let mut hands = vec![Vec::new(); n];
    for (k, card) in deck.into_iter().enumerate() {
        hands[(first + k) % n].push(card);
    }
    for hand in &mut hands {
        hand.sort();
    }
    hands
}

/// What lies on the trick: the value and how many cards of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Top {
    pub rank: u8,
    pub count: u8,
}

/// A round under way, for `n` players.
#[derive(Clone, Debug)]
pub struct Round {
    pub hands: Vec<Vec<Card>>,
    /// Whose turn.
    pub turn: usize,
    /// The trick: what lies on it (None: to lead), the cards last laid and by whom.
    pub top: Option<Top>,
    pub laid: Vec<Card>,
    pub leader: usize,
    /// Who passed in this trick (out of it until the next).
    pub passed: Vec<bool>,
    /// The order out of cards; the round is over when it holds everyone.
    pub out: Vec<usize>,
}

impl Round {
    /// A round from hands dealt (and exchanged), `lead` leading.
    pub fn new(hands: Vec<Vec<Card>>, lead: usize) -> Self {
        let n = hands.len();
        Self { hands, turn: lead, top: None, laid: Vec::new(), leader: lead, passed: vec![false; n], out: Vec::new() }
    }

    pub fn players(&self) -> usize {
        self.hands.len()
    }

    pub fn over(&self) -> bool {
        self.out.len() == self.players()
    }

    fn in_game(&self, p: usize) -> bool {
        !self.out.contains(&p)
    }

    /// Whether `p` may lay `cards` now (their turn, all held, one value, as many as the trick asks,
    /// stronger).
    pub fn legal(&self, p: usize, cards: &[Card]) -> bool {
        if self.over() || p != self.turn || cards.is_empty() || cards.len() > 4 {
            return false;
        }
        let rank = cards[0].rank;
        if cards.iter().any(|c| c.rank != rank || !self.hands[p].contains(c)) {
            return false;
        }
        let mut unique = cards.to_vec();
        unique.sort();
        unique.dedup();
        if unique.len() != cards.len() {
            return false;
        }
        match self.top {
            None => true,
            Some(top) => cards.len() == top.count as usize && rank > top.rank,
        }
    }

    /// `p` lays `cards` (false if not allowed).
    pub fn play(&mut self, p: usize, cards: &[Card]) -> bool {
        if !self.legal(p, cards) {
            return false;
        }
        self.hands[p].retain(|c| !cards.contains(c));
        self.top = Some(Top { rank: cards[0].rank, count: cards.len() as u8 });
        self.laid = cards.to_vec();
        self.leader = p;
        if self.hands[p].is_empty() {
            self.out.push(p);
        }
        // Only one left with cards: they're last.
        if self.out.len() + 1 == self.players()
            && let Some(last) = (0..self.players()).find(|&q| self.in_game(q))
        {
            self.out.push(last);
            return true;
        }
        if cards[0].rank == TWO {
            self.new_trick();
        } else {
            self.advance(p);
        }
        true
    }

    /// `p` passes: out of this trick (false if not their turn, or to lead).
    pub fn pass(&mut self, p: usize) -> bool {
        if self.over() || p != self.turn || self.top.is_none() {
            return false;
        }
        self.passed[p] = true;
        self.advance(p);
        true
    }

    /// The next one after `p` still in the trick; none left but the leader: a new trick.
    fn advance(&mut self, p: usize) {
        let n = self.players();
        let next = (1..=n).map(|k| (p + k) % n).find(|&q| q != self.leader && self.in_game(q) && !self.passed[q]);
        match next {
            Some(q) => self.turn = q,
            None => self.new_trick(),
        }
    }

    /// The trick cleared: the last who played leads, or the next one in game if they're out.
    fn new_trick(&mut self) {
        let n = self.players();
        self.top = None;
        self.passed = vec![false; n];
        self.turn = (0..n).map(|k| (self.leader + k) % n).find(|&q| self.in_game(q)).unwrap_or(self.leader);
    }

    /// The roles, by player, once over.
    pub fn roles(&self) -> Vec<Role> {
        let n = self.players();
        let mut roles = vec![Role::Neutral; n];
        for (place, &p) in self.out.iter().enumerate() {
            roles[p] = role(place, n);
        }
        roles
    }
}

/// How many cards a role gives (and gets back), and to whom: the Trou to the Président, the Vice-trou
/// to the Vice.
pub fn exchange_count(role: Role) -> usize {
    match role {
        Role::President | Role::Trou => 2,
        Role::Vice | Role::ViceTrou => 1,
        Role::Neutral => 0,
    }
}

/// Who `p` exchanges with (by the roles of the round before).
pub fn partner(roles: &[Role], p: usize) -> Option<usize> {
    let want = match roles[p] {
        Role::President => Role::Trou,
        Role::Trou => Role::President,
        Role::Vice => Role::ViceTrou,
        Role::ViceTrou => Role::Vice,
        Role::Neutral => return None,
    };
    roles.iter().position(|&r| r == want)
}

/// The best `k` cards of a hand (what the Trou and the Vice-trou must give).
pub fn best(hand: &[Card], k: usize) -> Vec<Card> {
    let mut sorted = hand.to_vec();
    sorted.sort();
    sorted.into_iter().rev().take(k).collect()
}

/// The cards the computer gives back, as Président or Vice: its weakest, single ones first (it keeps
/// its pairs).
pub fn give_back(hand: &[Card], k: usize) -> Vec<Card> {
    let mut sorted = hand.to_vec();
    sorted.sort_by_key(|c| (count(hand, c.rank), c.rank, c.suit));
    sorted.into_iter().take(k).collect()
}

/// The exchange: the lower ones give their best, the higher ones give back `given_back` (by player,
/// the cards they chose; the computer's are chosen with `give_back` when missing). The hands sorted.
pub fn exchange(hands: &mut [Vec<Card>], roles: &[Role], given_back: &[Option<Vec<Card>>]) {
    let n = hands.len();
    let mut moves: Vec<(usize, usize, Vec<Card>)> = Vec::new();
    for p in 0..n {
        let Some(q) = partner(roles, p) else { continue };
        let k = exchange_count(roles[p]);
        let cards = match roles[p] {
            Role::Trou | Role::ViceTrou => best(&hands[p], k),
            _ => given_back.get(p).cloned().flatten().filter(|c| c.len() == k && c.iter().all(|x| hands[p].contains(x))).unwrap_or_else(|| give_back(&hands[p], k)),
        };
        moves.push((p, q, cards));
    }
    for (from, to, cards) in moves {
        hands[from].retain(|c| !cards.contains(c));
        hands[to].extend(cards);
    }
    for hand in hands.iter_mut() {
        hand.sort();
    }
}

/// How many cards of `rank` in `hand`.
fn count(hand: &[Card], rank: u8) -> usize {
    hand.iter().filter(|c| c.rank == rank).count()
}

/// The values held, each with how many, weakest first.
fn groups(hand: &[Card]) -> Vec<(u8, usize)> {
    let mut out: Vec<(u8, usize)> = Vec::new();
    for c in hand {
        match out.last_mut() {
            Some((r, k)) if *r == c.rank => *k += 1,
            _ => out.push((c.rank, 1)),
        }
    }
    out
}

/// `k` cards of `rank` from `hand`.
fn take(hand: &[Card], rank: u8, k: usize) -> Vec<Card> {
    hand.iter().filter(|c| c.rank == rank).take(k).copied().collect()
}

/// The computer's move for `p`: the cards to lay, or None to pass.
/// Easy: any card that may go, and it passes now and then. Medium: the weakest that may go, a whole
/// value when it leads. Hard: as medium, but it doesn't break its pairs and sets or spend its 2s and
/// aces on a weak trick while it has many cards; with few values left it leads its strongest to keep
/// the hand.
pub fn ai_move(round: &Round, p: usize, level: Level, dice: &mut Dice) -> Option<Vec<Card>> {
    let hand = &round.hands[p];
    let groups = groups(hand);
    let Some(top) = round.top else {
        let lead = match level {
            Level::Easy => groups[dice.below(groups.len())],
            Level::Medium => groups[0],
            Level::Hard => {
                // A 2 first when only one other value is left: it closes the trick, then the rest goes.
                match groups.last() {
                    Some(&(TWO, _)) if groups.len() <= 2 => *groups.last().unwrap(),
                    _ => groups[0],
                }
            }
        };
        return Some(take(hand, lead.0, lead.1));
    };
    let need = top.count as usize;
    let options: Vec<(u8, usize)> = groups.iter().copied().filter(|&(r, k)| r > top.rank && k >= need).collect();
    if options.is_empty() {
        return None;
    }
    let pick = match level {
        Level::Easy => {
            if dice.below(4) == 0 {
                return None;
            }
            options[dice.below(options.len())]
        }
        Level::Medium => options[0],
        Level::Hard => {
            let few = hand.len() <= 5;
            let fine = |&&(r, k): &&(u8, usize)| {
                let breaks = k > need && r < 11;
                let wastes = r >= 11 && top.rank < 8 && !few;
                few || (!breaks && !wastes)
            };
            match options.iter().find(fine) {
                Some(&o) => o,
                // Nothing fits well: pass, unless it would keep it from ever going out.
                None if hand.len() > 8 => return None,
                None => options[0],
            }
        }
    };
    Some(take(hand, pick.0, need))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(rank: u8, suit: u8) -> Card {
        Card { rank, suit }
    }

    #[test]
    fn the_deal_gives_all_52_cards_sorted() {
        for n in 3..=6 {
            let hands = deal(n, 0, &mut Dice::new(7));
            assert_eq!(hands.iter().map(Vec::len).sum::<usize>(), 52);
            assert!(hands.iter().all(|h| h.windows(2).all(|w| w[0] < w[1])));
            assert!(hands.iter().all(|h| h.len() >= 52 / n));
        }
    }

    #[test]
    fn a_trick_asks_as_many_cards_stronger() {
        let mut r = Round::new(vec![vec![c(0, 0), c(0, 1), c(5, 0)], vec![c(3, 0), c(3, 1), c(4, 0)], vec![c(1, 0), c(2, 0), c(2, 1)]], 0);
        assert!(!r.legal(0, &[c(0, 0), c(5, 0)]), "one value only");
        assert!(r.play(0, &[c(0, 0), c(0, 1)]));
        assert!(!r.legal(1, &[c(4, 0)]), "a pair asked");
        assert!(r.play(1, &[c(3, 0), c(3, 1)]));
        assert!(!r.legal(2, &[c(2, 0), c(2, 1)]), "weaker");
        assert!(r.pass(2));
        // Player 0 passes too: player 1, the last who played, leads again.
        assert!(r.pass(0));
        assert_eq!((r.turn, r.top), (1, None));
    }

    #[test]
    fn a_two_closes_the_trick() {
        let mut r = Round::new(vec![vec![c(5, 0), c(7, 0)], vec![c(TWO, 0), c(1, 0)], vec![c(2, 0), c(3, 0)]], 0);
        assert!(r.play(0, &[c(5, 0)]));
        assert!(r.play(1, &[c(TWO, 0)]));
        assert_eq!((r.turn, r.top), (1, None));
    }

    #[test]
    fn the_order_out_gives_the_roles() {
        let mut r = Round::new(vec![vec![c(5, 0)], vec![c(1, 0), c(2, 0)], vec![c(3, 0), c(4, 0)], vec![c(0, 0), c(6, 0)]], 0);
        assert!(r.play(0, &[c(5, 0)]));
        assert_eq!(r.out, vec![0]);
        // Player 0 out: the trick goes on with the others, who can't beat a 8 but player 3.
        assert!(r.pass(1));
        assert!(r.pass(2));
        assert!(r.play(3, &[c(6, 0)]));
        assert_eq!((r.turn, r.top), (3, None));
        assert!(r.play(3, &[c(0, 0)]));
        assert_eq!(r.out, vec![0, 3]);
        assert!(r.play(1, &[c(1, 0)]));
        assert!(r.play(2, &[c(3, 0)]));
        assert!(r.pass(1));
        assert!(r.play(2, &[c(4, 0)]));
        assert!(r.over());
        assert_eq!(r.roles(), vec![Role::President, Role::Trou, Role::ViceTrou, Role::Vice]);
    }

    #[test]
    fn the_roles_by_number_of_players() {
        assert_eq!((0..3).map(|p| role(p, 3)).collect::<Vec<_>>(), vec![Role::President, Role::Neutral, Role::Trou]);
        assert_eq!((0..5).map(|p| role(p, 5)).collect::<Vec<_>>(), vec![Role::President, Role::Vice, Role::Neutral, Role::ViceTrou, Role::Trou]);
    }

    #[test]
    fn the_trou_gives_its_best_and_gets_back_the_presidents_choice() {
        let mut hands = vec![vec![c(0, 0), c(1, 0), c(11, 0)], vec![c(2, 0), c(TWO, 1), c(10, 1)], vec![c(4, 0), c(5, 0)]];
        let roles = [Role::President, Role::Trou, Role::Neutral];
        exchange(&mut hands, &roles, &[Some(vec![c(0, 0), c(1, 0)]), None, None]);
        assert_eq!(hands[0], vec![c(10, 1), c(11, 0), c(TWO, 1)]);
        assert_eq!(hands[1], vec![c(0, 0), c(1, 0), c(2, 0)]);
    }

    /// Whole rounds between computers: each ends, with everyone out once.
    #[test]
    fn computers_play_whole_rounds() {
        for (seed, n) in (0..60).zip((3..=6).cycle()) {
            let mut dice = Dice::new(seed + 11);
            let mut r = Round::new(deal(n, 0, &mut dice), 0);
            let level = [Level::Easy, Level::Medium, Level::Hard][seed as usize % 3];
            for _ in 0..2000 {
                if r.over() {
                    break;
                }
                let p = r.turn;
                match ai_move(&r, p, level, &mut dice) {
                    Some(cards) => assert!(r.play(p, &cards), "{cards:?} refused"),
                    None => assert!(r.pass(p)),
                }
            }
            assert!(r.over(), "a round never ended");
            let mut out = r.out.clone();
            out.sort();
            assert_eq!(out, (0..n).collect::<Vec<_>>());
        }
    }

    /// The hard computer goes out first more often than the easy ones.
    #[test]
    fn the_hard_computer_wins_more() {
        let mut presidents = 0;
        for seed in 0..200 {
            let mut dice = Dice::new(seed * 7 + 3);
            let mut r = Round::new(deal(4, 0, &mut dice), (seed % 4) as usize);
            while !r.over() {
                let p = r.turn;
                let level = if p == 0 { Level::Hard } else { Level::Easy };
                match ai_move(&r, p, level, &mut dice) {
                    Some(cards) => assert!(r.play(p, &cards)),
                    None => assert!(r.pass(p)),
                }
            }
            presidents += usize::from(r.out[0] == 0);
        }
        assert!(presidents > 70, "{presidents} / 200");
    }
}
