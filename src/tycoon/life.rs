//! The founder's private life: a personal account (fed by the founder's pay, the dividends, a sale),
//! a home and a car, a partner and children growing up, savings and investments, hobbies and luxuries,
//! the health and the stress. Both ways with the company: a crunch, a company in the red stress the
//! founder; a founder worn out works less (a burn-out: not at all for a while), a happy family makes
//! them work better, some hobbies bring offers or reputation, and the personal account can save the
//! company. Kept when the company is sold.

use serde::{Deserialize, Serialize};

use super::{Choice, Corp, EventDef, NewsKind, Tx};

/// A year of the family, in seconds (the children grow up by it, the investments earn by it).
pub const YEAR: f64 = 900.0;

pub struct Home {
    pub name: Tx,
    pub price: f64,
    /// € a second.
    pub upkeep: f64,
    pub comfort: f64,
    /// Bedrooms: the couple takes one, each child another.
    pub rooms: usize,
}

pub const HOMES: [Home; 8] = [
    Home { name: ("La chambre chez tes parents", "Your room at your parents'"), price: 0.0, upkeep: 0.0, comfort: 0.0, rooms: 1 },
    Home { name: ("Un studio", "A studio flat"), price: 15_000.0, upkeep: 0.05, comfort: 5.0, rooms: 1 },
    Home { name: ("Un T3 en ville", "A two-bedroom flat in town"), price: 120_000.0, upkeep: 0.2, comfort: 12.0, rooms: 3 },
    Home { name: ("Une maison avec jardin", "A house with a garden"), price: 450_000.0, upkeep: 0.6, comfort: 20.0, rooms: 5 },
    Home { name: ("Une villa avec piscine", "A villa with a pool"), price: 2_500_000.0, upkeep: 3.0, comfort: 30.0, rooms: 7 },
    Home { name: ("Un loft à Paris", "A loft in Paris"), price: 8_000_000.0, upkeep: 8.0, comfort: 35.0, rooms: 6 },
    Home { name: ("Un château", "A castle"), price: 40_000_000.0, upkeep: 30.0, comfort: 45.0, rooms: 15 },
    Home { name: ("Une île privée", "A private island"), price: 500_000_000.0, upkeep: 200.0, comfort: 60.0, rooms: 30 },
];

pub struct Car {
    pub name: Tx,
    pub price: f64,
    pub upkeep: f64,
    pub prestige: f64,
}

pub const CARS: [Car; 8] = [
    Car { name: ("Le bus", "The bus"), price: 0.0, upkeep: 0.0, prestige: 0.0 },
    Car { name: ("Une vieille Clio", "An old hatchback"), price: 2_000.0, upkeep: 0.01, prestige: 2.0 },
    Car { name: ("Une citadine neuve", "A new city car"), price: 18_000.0, upkeep: 0.03, prestige: 5.0 },
    Car { name: ("Un SUV familial", "A family SUV"), price: 45_000.0, upkeep: 0.06, prestige: 8.0 },
    Car { name: ("Une berline allemande", "A German saloon"), price: 90_000.0, upkeep: 0.1, prestige: 12.0 },
    Car { name: ("Une électrique de luxe", "A luxury electric car"), price: 140_000.0, upkeep: 0.08, prestige: 15.0 },
    Car { name: ("Une supercar italienne", "An Italian supercar"), price: 350_000.0, upkeep: 0.5, prestige: 25.0 },
    Car { name: ("Un hélicoptère", "A helicopter"), price: 3_000_000.0, upkeep: 4.0, prestige: 35.0 },
];

pub struct Hobby {
    pub id: &'static str,
    pub name: Tx,
    pub about: Tx,
    pub upkeep: f64,
    /// Stress it takes off.
    pub calm: f64,
}

/// At most this many hobbies at once (there's only so much time).
pub const MAX_HOBBIES: usize = 3;

pub const HOBBIES: [Hobby; 7] = [
    Hobby { id: "gym", name: ("Salle de sport", "Gym"), about: ("Stress −6, la santé remonte plus vite.", "Stress −6, health comes back faster."), upkeep: 0.02, calm: 6.0 },
    Hobby { id: "guitar", name: ("Guitare", "Guitar"), about: ("Stress −8.", "Stress −8."), upkeep: 0.005, calm: 8.0 },
    Hobby { id: "cooking", name: ("Cours de cuisine", "Cooking classes"), about: ("Stress −5, ton couple +8.", "Stress −5, your couple +8."), upkeep: 0.03, calm: 5.0 },
    Hobby { id: "golf", name: ("Golf", "Golf"), about: ("Stress −5. Le réseau : +10 % d'offres de contrats.", "Stress −5. The network: +10 % contract offers."), upkeep: 0.3, calm: 5.0 },
    Hobby { id: "sailing", name: ("Voile", "Sailing"), about: ("Stress −12, la santé remonte plus vite.", "Stress −12, health comes back faster."), upkeep: 1.0, calm: 12.0 },
    Hobby { id: "gaming", name: ("Jeux vidéo", "Video games"), about: ("Stress −6, ton couple −3.", "Stress −6, your couple −3."), upkeep: 0.01, calm: 6.0 },
    Hobby { id: "charity", name: ("Bénévolat", "Volunteering"), about: ("Stress −4. Réputation de la boîte +10 %.", "Stress −4. The company's reputation +10 %."), upkeep: 0.0, calm: 4.0 },
];

pub fn hobby(id: &str) -> Option<&'static Hobby> {
    HOBBIES.iter().find(|h| h.id == id)
}

pub struct Luxury {
    pub id: &'static str,
    pub name: Tx,
    pub about: Tx,
    pub price: f64,
    pub upkeep: f64,
    pub prestige: f64,
}

pub const LUXURIES: [Luxury; 7] = [
    Luxury { id: "watch", name: ("Une montre de luxe", "A luxury watch"), about: ("Pour regarder l'heure avec classe.", "To check the time with class."), price: 15_000.0, upkeep: 0.0, prestige: 5.0 },
    Luxury { id: "wine", name: ("Une cave à vin", "A wine cellar"), about: ("Stress −3.", "Stress −3."), price: 80_000.0, upkeep: 0.1, prestige: 4.0 },
    Luxury { id: "art", name: ("Une œuvre d'art", "A work of art"), about: ("Sa valeur monte avec le temps.", "Its value rises over time."), price: 500_000.0, upkeep: 0.5, prestige: 8.0 },
    Luxury { id: "yacht", name: ("Un yacht", "A yacht"), about: ("Les vacances détendent deux fois plus.", "Holidays relax twice as much."), price: 6_000_000.0, upkeep: 8.0, prestige: 25.0 },
    Luxury { id: "jet", name: ("Un jet privé", "A private jet"), about: ("Les vacances coûtent deux fois moins de temps.", "Holidays take half the time."), price: 45_000_000.0, upkeep: 40.0, prestige: 35.0 },
    Luxury { id: "foundation", name: ("Une fondation à ton nom", "A foundation in your name"), about: ("Réputation de la boîte +20 %.", "The company's reputation +20 %."), price: 100_000_000.0, upkeep: 50.0, prestige: 20.0 },
    Luxury { id: "space", name: ("Un vol dans l'espace", "A trip to space"), about: ("Vue d'en haut, tout paraît petit. Stress −10 pour toujours.", "Seen from up there, everything looks small. Stress −10 for good."), price: 300_000_000.0, upkeep: 0.0, prestige: 50.0 },
];

pub fn luxury(id: &str) -> Option<&'static Luxury> {
    LUXURIES.iter().find(|l| l.id == id)
}

/// (name, cost, length in seconds, stress taken off at once).
pub const HOLIDAYS: [(Tx, f64, f64, f64); 3] = [
    (("Un week-end", "A weekend"), 800.0, 300.0, 15.0),
    (("Une semaine au soleil", "A week in the sun"), 4_000.0, 900.0, 35.0),
    (("Le tour du monde", "A trip round the world"), 40_000.0, 2_700.0, 70.0),
];

/// A flat to let.
pub const RENTAL_PRICE: f64 = 180_000.0;
/// What a flat brings, a year.
pub const RENTAL_YIELD: f64 = 0.06;

/// (name, return a year, how much it moves a year).
pub const PLACEMENTS: [(Tx, f64, f64); 3] = [
    (("Livret d'épargne", "Savings account"), 0.02, 0.0),
    (("Actions (fonds indiciel)", "Shares (index fund)"), 0.07, 0.18),
    (("Crypto", "Crypto"), 0.12, 0.9),
];

/// The age a child leaves home (and can join the company).
pub const GROWN: f64 = 23.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Partner {
    pub name: String,
    /// 0 to 100: at 0, the couple splits up.
    pub happiness: f64,
    /// Seconds together.
    pub since: f64,
    pub married: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Kid {
    pub name: String,
    /// In seconds (`YEAR` a year).
    pub age: f64,
    #[serde(default)]
    pub private_school: bool,
    /// Grown up and gone (no more costs).
    #[serde(default)]
    pub grown: bool,
}

impl Kid {
    pub fn years(&self) -> f64 {
        self.age / YEAR
    }

    /// € a second.
    pub fn cost(&self) -> f64 {
        let y = self.years();
        let school = if self.private_school { 1.0 } else { 0.0 };
        if self.grown {
            0.0
        } else if y < 3.0 {
            0.06
        } else if y < 11.0 {
            0.04 + 0.15 * school
        } else if y < 18.0 {
            0.08 + 0.25 * school
        } else {
            0.25
        }
    }

    pub fn stage(&self) -> Tx {
        let y = self.years();
        if self.grown {
            ("parti·e de la maison", "left home")
        } else if y < 3.0 {
            ("bébé", "baby")
        } else if y < 11.0 {
            ("à l'école", "at school")
        } else if y < 18.0 {
            ("ado", "teenager")
        } else {
            ("étudiant·e", "student")
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Life {
    pub account: f64,
    pub home: u8,
    pub car: u8,
    pub partner: Option<Partner>,
    pub kids: Vec<Kid>,
    pub savings: f64,
    pub stocks: f64,
    pub crypto: f64,
    pub rentals: u32,
    pub hobbies: Vec<String>,
    pub luxuries: Vec<String>,
    /// 0 to 100.
    pub health: f64,
    /// 0 to 100.
    pub stress: f64,
    /// 0: little (the founder works more), 1: normal, 2: a lot (the family is happier).
    pub family_time: u8,
    /// Seconds of holiday left (the founder doesn't work).
    pub holiday: f64,
    /// Seconds of burn-out left (the founder doesn't work).
    pub burnout: f64,
    /// Seconds worked since the last holiday.
    pub worked: f64,
    /// The mortgage left, and what it takes a second.
    pub loan: f64,
    pub loan_pay: f64,
    /// Seconds single (a meeting comes after a while).
    pub single: f64,
    /// What the personal account takes and gets a second, smoothed (shown).
    pub income: f64,
    pub spend: f64,
}

impl Default for Life {
    fn default() -> Self {
        Self {
            account: 500.0,
            home: 0,
            car: 0,
            partner: None,
            kids: Vec::new(),
            savings: 0.0,
            stocks: 0.0,
            crypto: 0.0,
            rentals: 0,
            hobbies: Vec::new(),
            luxuries: Vec::new(),
            health: 85.0,
            stress: 20.0,
            family_time: 1,
            holiday: 0.0,
            burnout: 0.0,
            worked: 0.0,
            loan: 0.0,
            loan_pay: 0.0,
            single: 0.0,
            income: 0.0,
            spend: 0.0,
        }
    }
}

impl Life {
    pub fn home(&self) -> &'static Home {
        &HOMES[(self.home as usize).min(HOMES.len() - 1)]
    }

    pub fn car(&self) -> &'static Car {
        &CARS[(self.car as usize).min(CARS.len() - 1)]
    }

    pub fn has(&self, luxury: &str) -> bool {
        self.luxuries.iter().any(|l| l == luxury)
    }

    pub fn does(&self, hobby: &str) -> bool {
        self.hobbies.iter().any(|h| h == hobby)
    }

    pub fn kids_home(&self) -> usize {
        self.kids.iter().filter(|k| !k.grown).count()
    }

    /// The founder isn't at work (a holiday, a burn-out).
    pub fn away(&self) -> bool {
        self.holiday > 0.0 || self.burnout > 0.0
    }

    pub fn prestige(&self) -> f64 {
        self.car().prestige + self.luxuries.iter().filter_map(|l| luxury(l)).map(|l| l.prestige).sum::<f64>() + self.home().comfort * 0.5
    }

    /// What it costs a second to live.
    pub fn costs(&self) -> f64 {
        let hobbies: f64 = self.hobbies.iter().filter_map(|h| hobby(h)).map(|h| h.upkeep).sum();
        let luxuries: f64 = self.luxuries.iter().filter_map(|l| luxury(l)).map(|l| l.upkeep).sum();
        let kids: f64 = self.kids.iter().map(Kid::cost).sum();
        let partner = if self.partner.is_some() { 0.03 } else { 0.0 };
        self.home().upkeep + self.car().upkeep + hobbies + luxuries + kids + partner + if self.loan > 0.0 { self.loan_pay } else { 0.0 }
    }

    /// What the rentals bring a second.
    pub fn rent_income(&self) -> f64 {
        self.rentals as f64 * RENTAL_PRICE * RENTAL_YIELD / YEAR
    }

    /// Everything owned, the mortgage taken off.
    pub fn net_worth(&self) -> f64 {
        let luxuries: f64 = self.luxuries.iter().filter_map(|l| luxury(l)).map(|l| l.price * if l.id == "art" { 1.0 } else { 0.6 }).sum();
        self.account + self.savings + self.stocks + self.crypto + self.rentals as f64 * RENTAL_PRICE + self.home().price * 0.8 + self.car().price * 0.5 + luxuries - self.loan
    }

    /// The founder's work: ×0 away, less worn out or spending time with the family, more when happy.
    pub fn work_factor(&self) -> f64 {
        if self.away() {
            return 0.0;
        }
        let health = 0.6 + 0.4 * self.health / 100.0;
        let time = [1.1, 1.0, 0.85][self.family_time.min(2) as usize];
        let happy = self.partner.as_ref().map_or(0.0, |p| ((p.happiness - 50.0) / 50.0).max(0.0) * 0.1);
        health * time * (1.0 + happy)
    }
}

const PARTNER_NAMES: [&str; 24] = [
    "Alex", "Camille", "Charlie", "Sasha", "Morgan", "Robin", "Claude", "Dominique", "Eden", "Noa", "Lou", "Andrea",
    "Jules", "Ange", "Maxime", "Kim", "Ali", "Sam", "Marion", "Yael", "Gaby", "Céleste", "Loïc", "Inès",
];
const KID_NAMES: [&str; 24] = [
    "Léo", "Jade", "Gabriel", "Louise", "Raphaël", "Ambre", "Arthur", "Alba", "Louis", "Rose", "Jules", "Romy",
    "Adam", "Mia", "Maël", "Anna", "Lucas", "Iris", "Hugo", "Lina", "Noé", "Zoé", "Sacha", "Nina",
];

impl Corp {
    fn life_spend(&mut self, amount: f64) -> bool {
        if amount > self.life.account {
            return false;
        }
        self.life.account -= amount;
        true
    }

    // --- what the player does with their life (each one counted)

    pub fn set_family_time(&mut self, v: u8) {
        if v <= 2 && v != self.life.family_time {
            self.life.family_time = v;
            self.acted();
        }
    }

    /// The home bought (the old one sold 80 % of its price). With a mortgage: a fifth down, the rest
    /// paid over four years, 10 % more.
    pub fn home_cost(&self, k: usize, loan: bool) -> f64 {
        let Some(h) = HOMES.get(k) else { return f64::INFINITY };
        let resale = self.life.home().price * 0.8 - self.life.loan;
        (if loan { h.price * 0.2 } else { h.price } - resale).max(0.0)
    }

    pub fn can_buy_home(&self, k: usize, loan: bool) -> bool {
        k < HOMES.len() && k != self.life.home as usize && self.life.account >= self.home_cost(k, loan) && (k as u8 > self.life.home || self.life.kids_home() < HOMES[k].rooms)
    }

    pub fn buy_home(&mut self, k: usize, loan: bool) -> bool {
        if !self.can_buy_home(k, loan) {
            return false;
        }
        let resale = self.life.home().price * 0.8 - self.life.loan;
        let price = HOMES[k].price;
        if loan {
            self.life.account -= (price * 0.2 - resale).max(0.0);
            self.life.account += (resale - price * 0.2).max(0.0);
            self.life.loan = price * 0.8 * 1.1;
            self.life.loan_pay = self.life.loan / (YEAR * 4.0);
        } else {
            self.life.account += resale - price;
            self.life.loan = 0.0;
            self.life.loan_pay = 0.0;
        }
        self.life.home = k as u8;
        self.say(NewsKind::Life, "", 0.0, k as f64);
        self.acted();
        true
    }

    /// The mortgage paid off at once.
    pub fn repay_loan(&mut self) -> bool {
        if self.life.loan <= 0.0 || self.life.account < self.life.loan {
            return false;
        }
        self.life.account -= self.life.loan;
        self.life.loan = 0.0;
        self.life.loan_pay = 0.0;
        self.acted();
        true
    }

    pub fn car_cost(&self, k: usize) -> f64 {
        CARS.get(k).map_or(f64::INFINITY, |c| (c.price - self.life.car().price * 0.5).max(0.0))
    }

    pub fn buy_car(&mut self, k: usize) -> bool {
        if k >= CARS.len() || k == self.life.car as usize {
            return false;
        }
        let cost = CARS[k].price - self.life.car().price * 0.5;
        if cost > self.life.account {
            return false;
        }
        self.life.account -= cost;
        self.life.car = k as u8;
        self.say(NewsKind::Life, "", 1.0, k as f64);
        self.acted();
        true
    }

    /// Together long enough and happy: the wedding (0: small, 1: big).
    pub fn wedding_cost(&self, big: bool) -> f64 {
        if big { 80_000.0 + self.life.account * 0.02 } else { 15_000.0 }
    }

    pub fn can_marry(&self) -> bool {
        self.life.partner.as_ref().is_some_and(|p| !p.married && p.since >= 1_200.0 && p.happiness >= 60.0)
    }

    pub fn marry(&mut self, big: bool) -> bool {
        let cost = self.wedding_cost(big);
        if !self.can_marry() || !self.life_spend(cost) {
            return false;
        }
        if let Some(p) = self.life.partner.as_mut() {
            p.married = true;
            p.happiness = (p.happiness + if big { 25.0 } else { 15.0 }).min(100.0);
            let name = p.name.clone();
            self.life.stress = (self.life.stress - if big { 10.0 } else { 5.0 }).max(0.0);
            self.say(NewsKind::Life, &name, 2.0, 0.0);
        }
        self.acted();
        true
    }

    /// A child: a couple, a room free, and the partner willing.
    pub fn can_have_kid(&self) -> bool {
        self.life.partner.as_ref().is_some_and(|p| p.happiness >= 50.0) && self.life.kids_home() + 1 < self.life.home().rooms && self.life.kids.len() < 6
    }

    pub fn have_kid(&mut self) -> bool {
        if !self.can_have_kid() {
            return false;
        }
        let taken: Vec<String> = self.life.kids.iter().map(|k| k.name.clone()).collect();
        let free: Vec<&str> = KID_NAMES.iter().copied().filter(|n| !taken.iter().any(|t| t == n)).collect();
        let name = free[self.pick(free.len())].to_owned();
        self.life.kids.push(Kid { name: name.clone(), age: 0.0, private_school: false, grown: false });
        if let Some(p) = self.life.partner.as_mut() {
            p.happiness = (p.happiness + 10.0).min(100.0);
        }
        self.life.stress = (self.life.stress + 8.0).min(100.0);
        self.say(NewsKind::Life, &name, 3.0, 0.0);
        self.acted();
        true
    }

    pub fn set_school(&mut self, k: usize, private: bool) {
        if let Some(kid) = self.life.kids.get_mut(k)
            && kid.private_school != private
            && !kid.grown
        {
            kid.private_school = private;
            self.acted();
        }
    }

    /// A holiday: the founder away, the stress down (the partner comes along).
    pub fn holiday_cost(&self, k: usize) -> f64 {
        HOLIDAYS.get(k).map_or(f64::INFINITY, |h| h.1 * (1.0 + self.life.kids_home() as f64 * 0.3) * if self.life.partner.is_some() { 1.6 } else { 1.0 })
    }

    pub fn can_holiday(&self, k: usize) -> bool {
        k < HOLIDAYS.len() && !self.life.away() && self.life.account >= self.holiday_cost(k)
    }

    pub fn go_on_holiday(&mut self, k: usize) -> bool {
        if !self.can_holiday(k) {
            return false;
        }
        let (_, _, secs, calm) = HOLIDAYS[k];
        self.life.account -= self.holiday_cost(k);
        let yacht = if self.life.has("yacht") { 2.0 } else { 1.0 };
        let jet = if self.life.has("jet") { 0.5 } else { 1.0 };
        self.life.holiday = secs * jet;
        self.life.worked = 0.0;
        self.life.stress = (self.life.stress - calm * yacht).max(0.0);
        if let Some(p) = self.life.partner.as_mut() {
            p.happiness = (p.happiness + calm * 0.4).min(100.0);
        }
        self.say(NewsKind::Life, "", 4.0, k as f64);
        self.acted();
        true
    }

    /// Money put into a placement (0 savings, 1 shares, 2 crypto), or taken out (below 0).
    pub fn place(&mut self, kind: usize, amount: f64) -> bool {
        let slot = match kind {
            0 => &mut self.life.savings,
            1 => &mut self.life.stocks,
            2 => &mut self.life.crypto,
            _ => return false,
        };
        let amount = if amount >= 0.0 { amount.min(self.life.account) } else { -((-amount).min(*slot)) };
        if amount.abs() < 0.01 {
            return false;
        }
        *slot += amount;
        self.life.account -= amount;
        self.acted();
        true
    }

    pub fn buy_rental(&mut self) -> bool {
        if !self.life_spend(RENTAL_PRICE) {
            return false;
        }
        self.life.rentals += 1;
        self.acted();
        true
    }

    pub fn sell_rental(&mut self) -> bool {
        if self.life.rentals == 0 {
            return false;
        }
        self.life.rentals -= 1;
        self.life.account += RENTAL_PRICE * 0.95;
        self.acted();
        true
    }

    pub fn toggle_hobby(&mut self, id: &str) {
        if hobby(id).is_none() {
            return;
        }
        if let Some(k) = self.life.hobbies.iter().position(|h| h == id) {
            self.life.hobbies.remove(k);
        } else if self.life.hobbies.len() < MAX_HOBBIES {
            self.life.hobbies.push(id.to_owned());
        } else {
            return;
        }
        self.acted();
    }

    pub fn buy_luxury(&mut self, id: &str) -> bool {
        let Some(l) = luxury(id) else { return false };
        if self.life.has(id) || !self.life_spend(l.price) {
            return false;
        }
        self.life.luxuries.push(id.to_owned());
        if id == "space" {
            self.life.stress = (self.life.stress - 30.0).max(0.0);
        }
        self.say(NewsKind::Life, "", 5.0, LUXURIES.iter().position(|x| x.id == id).unwrap_or(0) as f64);
        self.acted();
        true
    }

    pub fn sell_luxury(&mut self, id: &str) {
        let Some(l) = luxury(id) else { return };
        if let Some(k) = self.life.luxuries.iter().position(|x| x == id) {
            self.life.luxuries.remove(k);
            self.life.account += l.price * if id == "art" { 1.0 } else { 0.6 };
            self.acted();
        }
    }

    // --- the life going on

    /// Each step: the money, the investments, the family, the health.
    pub(super) fn run_life(&mut self, dt: f64, online: bool) {
        let crunch = self.policy.hours == 2;
        let company_red = self.cash < 0.0;
        let stage = self.stage as f64;
        // Some randomness first (the markets).
        let (r1, r2) = (self.rand() - 0.5, self.rand() - 0.5);
        let l = &mut self.life;

        // The money.
        let costs = l.costs() * dt;
        let rent = l.rent_income() * dt;
        l.account += rent - costs;
        if l.loan > 0.0 {
            l.loan = (l.loan - l.loan_pay * dt).max(0.0);
            if l.loan == 0.0 {
                l.loan_pay = 0.0;
            }
        }
        let a = 1.0 - (-dt / 300.0).exp();
        l.spend += (costs / dt - l.spend) * a;
        l.income *= 1.0 - a;
        l.income += rent / dt * a;
        // The placements: a year's return spread, and the markets moving (a uniform draw of the same spread
        // as a normal one).
        let years = dt / YEAR;
        l.savings *= 1.0 + PLACEMENTS[0].1 * years;
        l.stocks *= (PLACEMENTS[1].1 * years + PLACEMENTS[1].2 * years.sqrt() * r1 * 3.46).exp();
        l.crypto *= (PLACEMENTS[2].1 * years + PLACEMENTS[2].2 * years.sqrt() * r2 * 3.46).exp();

        // The family grows up.
        for k in &mut l.kids {
            if !k.grown {
                k.age += dt;
            }
        }

        // The stress and the health.
        let calm: f64 = l.hobbies.iter().filter_map(|h| hobby(h)).map(|h| h.calm).sum::<f64>() + if l.has("wine") { 3.0 } else { 0.0 } + if l.has("space") { 10.0 } else { 0.0 };
        let couple = l.partner.as_ref().map_or(0.0, |p| if p.happiness > 60.0 { 5.0 } else if p.happiness < 30.0 { -8.0 } else { 0.0 });
        let target = if l.holiday > 0.0 {
            5.0
        } else if l.burnout > 0.0 {
            30.0
        } else {
            25.0 + if crunch { 25.0 } else if self.policy.hours == 0 { -8.0 } else { 0.0 }
                + if company_red { 25.0 } else { 0.0 }
                + if l.account < 0.0 { 20.0 } else { 0.0 }
                + [10.0, 0.0, -10.0][l.family_time.min(2) as usize]
                + stage * 2.0
                + (l.worked / (YEAR * 2.0) * 10.0).min(25.0)
                + l.kids_home() as f64 * 2.0
                - l.home().comfort * 0.3
                - calm
                - couple
        };
        l.stress += (target.clamp(0.0, 100.0) - l.stress) * (1.0 - (-dt / 240.0).exp());
        let sport = if l.does("gym") || l.does("sailing") { 0.01 } else { 0.0 };
        if l.burnout > 0.0 {
            l.health = (l.health + 0.06 * dt).min(100.0);
        } else if l.stress > 70.0 {
            l.health = (l.health - (l.stress - 70.0) * 0.004 * dt).max(0.0);
        } else {
            l.health = (l.health + (0.01 + sport) * dt).min(100.0);
        }
        if l.holiday > 0.0 {
            l.holiday = (l.holiday - dt).max(0.0);
        } else if l.burnout > 0.0 {
            l.burnout = (l.burnout - dt).max(0.0);
        } else {
            l.worked += dt;
        }

        // The couple.
        let prestige = l.prestige();
        let account_red = l.account < 0.0;
        let (comfort, family_time, stress, on_holiday) = (l.home().comfort, l.family_time, l.stress, l.holiday > 0.0);
        let hobby_couple = if l.does("cooking") { 8.0 } else { 0.0 } - if l.does("gaming") { 3.0 } else { 0.0 };
        match l.partner.as_mut() {
            Some(p) => {
                p.since += dt;
                let target = 55.0 + comfort * 0.5 + (prestige * 0.1).min(10.0) + [-25.0, 0.0, 15.0][family_time.min(2) as usize] - stress * 0.3 + if p.married { 5.0 } else { 0.0 } + if on_holiday { 20.0 } else { 0.0 } - if account_red { 20.0 } else { 0.0 } - if crunch { 10.0 } else { 0.0 } + hobby_couple;
                p.happiness += (target.clamp(0.0, 100.0) - p.happiness) * (1.0 - (-dt / 600.0).exp());
                p.happiness = p.happiness.clamp(0.0, 100.0);
            }
            None => l.single += dt,
        }

        // Burn-out, the couple splitting up, the children leaving home: while Ronnie is open only.
        if !online {
            return;
        }
        if self.life.burnout <= 0.0 && self.life.health < 15.0 {
            self.life.burnout = 1_200.0;
            self.life.holiday = 0.0;
            self.say(NewsKind::Life, "", 6.0, 0.0);
        }
        if self.life.partner.as_ref().is_some_and(|p| p.happiness <= 1.0) {
            self.divorce();
        }
        let grown: Vec<usize> = (0..self.life.kids.len()).filter(|&k| !self.life.kids[k].grown && self.life.kids[k].years() >= GROWN).collect();
        for k in grown {
            self.life.kids[k].grown = true;
            let private = self.life.kids[k].private_school;
            let name = self.life.kids[k].name.clone();
            // They knock at the company's door: a candidate.
            let jobs = ["front", "back", "mobile", "design", "data", "fullstack"];
            let job = jobs[self.pick(jobs.len())];
            let mut e = self.make_employee(job, if private { 5 } else { 3 });
            e.name = name.clone();
            e.trait_id = Some("motivated".into());
            self.candidates.insert(0, e);
            self.say(NewsKind::Life, &name, 7.0, 0.0);
        }
    }

    /// The couple splits up: half of everything goes.
    fn divorce(&mut self) {
        let Some(p) = self.life.partner.take() else { return };
        let l = &mut self.life;
        if p.married {
            l.account *= 0.5;
            l.savings *= 0.5;
            l.stocks *= 0.5;
            l.crypto *= 0.5;
            l.rentals /= 2;
        }
        l.stress = (l.stress + 30.0).min(100.0);
        l.single = 0.0;
        self.mood -= 3.0;
        self.say(NewsKind::Life, &p.name, 8.0, if p.married { 1.0 } else { 0.0 });
    }

    /// A partner met (an event).
    fn meet(&mut self) {
        let name = PARTNER_NAMES[self.pick(PARTNER_NAMES.len())].to_owned();
        self.life.partner = Some(Partner { name: name.clone(), happiness: 65.0, since: 0.0, married: false });
        self.life.single = 0.0;
        self.say(NewsKind::Life, &name, 9.0, 0.0);
    }

    fn couple(&mut self, by: f64) {
        if let Some(p) = self.life.partner.as_mut() {
            p.happiness = (p.happiness + by).clamp(0.0, 100.0);
        }
    }

    fn stress_by(&mut self, by: f64) {
        self.life.stress = (self.life.stress + by).clamp(0.0, 100.0);
    }

    /// What a personal surprise costs: the founder's standing (a bit of the account, the company's size).
    fn life_unit(&self) -> f64 {
        (500.0 * (1.0 + self.stage as f64).powi(2)).max(self.life.account * 0.02)
    }
}

/// The founder's life's surprises (with the company's, one at a time).
pub const LIFE_EVENTS: [EventDef; 12] = [
    EventDef {
        id: "meet",
        title: ("Coup de foudre en terrasse", "Love at first sight"),
        text: ("En sortant du bureau, tu croises un regard. On t'invite à prendre un verre.", "Leaving the office, your eyes meet someone's. You're invited for a drink."),
        fits: |c| c.life.partner.is_none() && c.life.single >= 600.0 && c.life.stress < 85.0,
        choices: &[
            Choice { label: ("Accepter le verre", "Accept the drink"), outcomes: &[("Un verre, puis un dîner, puis… vous ne vous quittez plus.", "A drink, then dinner, then… you're inseparable.")], apply: |c| {
                c.meet();
                0
            } },
            Choice { label: ("Pas le temps, la boîte d'abord", "No time, the company first"), outcomes: &[("Ce sera pour une autre fois.", "Maybe another time.")], apply: |c| {
                c.life.single = 0.0;
                0
            } },
        ],
    },
    EventDef {
        id: "anniversary",
        title: ("Votre anniversaire", "Your anniversary"),
        text: ("Ça fait un moment que vous êtes ensemble. Ce soir, c'est votre anniversaire.", "You've been together for a while. Tonight is your anniversary."),
        fits: |c| c.life.partner.as_ref().is_some_and(|p| p.since >= 900.0),
        choices: &[
            Choice { label: ("Dîner dans un restaurant étoilé", "Dinner at a starred restaurant"), outcomes: &[("Une soirée parfaite.", "A perfect evening.")], apply: |c| {
                c.life.account -= c.life_unit() * 0.6;
                c.couple(12.0);
                0
            } },
            Choice { label: ("Une soirée tranquille à la maison", "A quiet night in"), outcomes: &[("Simple et doux.", "Simple and sweet.")], apply: |c| {
                c.couple(5.0);
                0
            } },
            Choice { label: ("Oups… tu as oublié", "Oops… you forgot"), outcomes: &[("Ambiance glaciale à la maison.", "Freezing mood at home.")], apply: |c| {
                c.couple(-15.0);
                0
            } },
        ],
    },
    EventDef {
        id: "crisis",
        title: ("Ça ne va plus à la maison", "Trouble at home"),
        text: ("Tu n'es jamais là, et quand tu es là, tu penses au boulot. Il faut faire quelque chose.", "You're never there, and when you are, you think about work. Something has to change."),
        fits: |c| c.life.partner.as_ref().is_some_and(|p| p.happiness < 30.0),
        choices: &[
            Choice { label: ("Une thérapie de couple", "Couples therapy"), outcomes: &[("Ça aide. Vous vous parlez de nouveau.", "It helps. You're talking again.")], apply: |c| {
                c.life.account -= c.life_unit();
                c.couple(25.0);
                0
            } },
            Choice { label: ("Partir en vacances à deux", "A holiday for two"), outcomes: &[("Loin du bureau, vous vous retrouvez.", "Away from the office, you find each other again.")], apply: |c| {
                c.life.account -= c.life_unit() * 1.5;
                c.life.holiday = c.life.holiday.max(600.0);
                c.life.worked = 0.0;
                c.couple(30.0);
                0
            } },
            Choice { label: ("On verra plus tard", "We'll see later"), outcomes: &[("La porte claque.", "The door slams.")], apply: |c| {
                c.couple(-10.0);
                0
            } },
        ],
    },
    EventDef {
        id: "kid_sick",
        title: ("Un enfant malade", "A sick child"),
        text: ("L'école appelle : un de tes enfants a de la fièvre. Il faut aller le chercher.", "The school calls: one of your children has a fever. Someone has to pick them up."),
        fits: |c| c.life.kids.iter().any(|k| !k.grown && k.years() < 14.0),
        choices: &[
            Choice { label: ("Y aller et rester à la maison", "Go, and stay home"), outcomes: &[("Une journée de câlins et de dessins animés.", "A day of cuddles and cartoons.")], apply: |c| {
                c.life.holiday = c.life.holiday.max(240.0);
                c.couple(6.0);
                c.stress_by(-4.0);
                0
            } },
            Choice { label: ("Appeler une nounou", "Call a nanny"), outcomes: &[("La nounou gère. Toi, tu culpabilises un peu.", "The nanny handles it. You feel a bit guilty.")], apply: |c| {
                c.life.account -= c.life_unit() * 0.3;
                c.stress_by(5.0);
                0
            } },
        ],
    },
    EventDef {
        id: "school_report",
        title: ("Le bulletin scolaire", "The school report"),
        text: ("Les notes ne sont pas fameuses. La maîtresse parle d'un « potentiel inexploité ».", "The grades aren't great. The teacher talks of \"untapped potential\"."),
        fits: |c| c.life.kids.iter().any(|k| !k.grown && (6.0..18.0).contains(&k.years()) && !k.private_school),
        choices: &[
            Choice { label: ("Passer à l'école privée", "Switch to a private school"), outcomes: &[("Uniforme, latin et devoirs. Plus tard, un meilleur niveau.", "Uniform, Latin and homework. Later, a better level.")], apply: |c| {
                if let Some(k) = c.life.kids.iter_mut().find(|k| !k.grown && (6.0..18.0).contains(&k.years()) && !k.private_school) {
                    k.private_school = true;
                }
                0
            } },
            Choice { label: ("Des cours particuliers", "Private tutoring"), outcomes: &[("Les notes remontent.", "The grades go up.")], apply: |c| {
                c.life.account -= c.life_unit() * 0.5;
                c.couple(3.0);
                0
            } },
            Choice { label: ("Il·elle se débrouillera", "They'll manage"), outcomes: &[("On verra au prochain trimestre.", "We'll see next term.")], apply: |_| 0 },
        ],
    },
    EventDef {
        id: "fender",
        title: ("Un accrochage", "A fender bender"),
        text: ("Un rétroviseur, une portière, un constat à remplir sur un capot.", "A wing mirror, a door, a form to fill in on a bonnet."),
        fits: |c| c.life.car > 0,
        choices: &[
            Choice { label: ("Faire réparer", "Get it fixed"), outcomes: &[("Comme neuve.", "Good as new.")], apply: |c| {
                c.life.account -= c.life.car().price * 0.15;
                0
            } },
            Choice { label: ("Rouler comme ça", "Drive it as it is"), outcomes: &[("Ça fait du caractère.", "It adds character.")], apply: |c| {
                c.couple(-2.0);
                c.stress_by(2.0);
                0
            } },
        ],
    },
    EventDef {
        id: "doctor",
        title: ("Le médecin s'inquiète", "The doctor is worried"),
        text: ("Tension, insomnies, mâchoire serrée : « Il faut lever le pied. »", "Blood pressure, sleepless nights, clenched jaw: \"You need to slow down.\""),
        fits: |c| c.life.stress > 65.0 && !c.life.away(),
        choices: &[
            Choice { label: ("Prendre une semaine", "Take a week off"), outcomes: &[("Une semaine sans mails. Ça fait du bien.", "A week without e-mail. It feels good.")], apply: |c| {
                c.life.holiday = 900.0;
                c.life.worked = 0.0;
                c.stress_by(-25.0);
                0
            } },
            Choice { label: ("Plus tard, là c'est pas possible", "Later, not now"), outcomes: &[("Le corps, lui, n'attend pas.", "Your body, though, won't wait.")], apply: |c| {
                c.life.health = (c.life.health - 12.0).max(0.0);
                0
            } },
        ],
    },
    EventDef {
        id: "inheritance",
        title: ("Un héritage inattendu", "An unexpected inheritance"),
        text: ("Un grand-oncle que tu n'as vu qu'une fois t'a couché·e sur son testament.", "A great-uncle you met only once put you in his will."),
        fits: |_| true,
        choices: &[
            Choice { label: ("Le placer sur le livret", "Put it in savings"), outcomes: &[("Ça dort au chaud.", "It sleeps safe and warm.")], apply: |c| {
                c.life.savings += c.life_unit() * 4.0;
                0
            } },
            Choice { label: ("Se faire plaisir", "Treat yourself"), outcomes: &[("Un beau voyage, des cadeaux pour tout le monde.", "A nice trip, presents for everyone.")], apply: |c| {
                c.couple(10.0);
                c.stress_by(-10.0);
                0
            } },
        ],
    },
    EventDef {
        id: "cousin_crypto",
        title: ("Ton cousin et sa crypto", "Your cousin and his crypto"),
        text: ("« C'est le prochain Bitcoin, je te jure. » Il a déjà acheté une Lamborghini… en location.", "\"It's the next Bitcoin, I swear.\" He's already bought a Lamborghini… leased."),
        fits: |c| c.life.account > 5_000.0,
        choices: &[
            Choice { label: ("Mettre 10 % de ton compte", "Put in 10 % of your account"), outcomes: &[("La fusée décolle !", "To the moon!"), ("Le jeton ne vaut plus rien.", "The token is worth nothing now.")], apply: |c| {
                let stake = c.life.account * 0.1;
                c.life.account -= stake;
                if c.chance(0.35) {
                    c.life.account += stake * 4.0;
                    0
                } else {
                    1
                }
            } },
            Choice { label: ("Non merci", "No thanks"), outcomes: &[("Le cousin te traite de boomer.", "Your cousin calls you a boomer.")], apply: |_| 0 },
        ],
    },
    EventDef {
        id: "reunion",
        title: ("Les retrouvailles de promo", "The class reunion"),
        text: ("Vingt ans après, tout le monde demande ce que devient ta boîte. Un ancien camarade cherche du travail.", "Twenty years on, everyone asks how your company is doing. An old classmate is looking for a job."),
        fits: |c| c.stage >= 1,
        choices: &[
            Choice { label: ("Lui proposer un poste", "Offer them a job"), outcomes: &[("Ton ancien camarade est dans les candidats.", "Your old classmate is among the candidates.")], apply: |c| {
                let jobs = ["front", "back", "design", "mobile"];
                let job = jobs[c.pick(jobs.len())];
                let level = (2 + c.stage).min(super::MAX_LEVEL);
                let mut e = c.make_employee(job, level);
                e.trait_id = Some("social".into());
                c.candidates.insert(0, e);
                0
            } },
            Choice { label: ("Juste profiter de la soirée", "Just enjoy the evening"), outcomes: &[("Ça fait du bien de rire du bon vieux temps.", "It's good to laugh about the old days.")], apply: |c| {
                c.stress_by(-8.0);
                0
            } },
        ],
    },
    EventDef {
        id: "midlife",
        title: ("La crise de la quarantaine", "The midlife crisis"),
        text: ("Tu te surprends à regarder des annonces de voitures de sport et des retraites de méditation.", "You catch yourself browsing sports-car ads and meditation retreats."),
        fits: |c| c.stage >= 3 && c.life.car < 6,
        choices: &[
            Choice { label: ("La supercar italienne", "The Italian supercar"), outcomes: &[("Vroum. Le voisinage a entendu.", "Vroom. The neighbours heard."), ("Ton compte n'est pas d'accord.", "Your bank account disagrees.")], apply: |c| {
                if c.buy_car(6) {
                    c.stress_by(-12.0);
                    0
                } else {
                    1
                }
            } },
            Choice { label: ("Une retraite de méditation", "A meditation retreat"), outcomes: &[("Ommm.", "Ommm.")], apply: |c| {
                c.life.account -= c.life_unit();
                c.life.holiday = c.life.holiday.max(450.0);
                c.life.worked = 0.0;
                c.stress_by(-30.0);
                0
            } },
            Choice { label: ("Rien, ça va passer", "Nothing, it'll pass"), outcomes: &[("Ça passe. Lentement.", "It passes. Slowly.")], apply: |_| 0 },
        ],
    },
    EventDef {
        id: "tax_personal",
        title: ("Contrôle fiscal… perso", "A personal tax audit"),
        text: ("Le fisc s'intéresse à tes revenus. Ton comptable sue un peu.", "The taxman is looking at your income. Your accountant is sweating a bit."),
        fits: |c| c.life.account > 20_000.0,
        choices: &[
            Choice { label: ("Payer sans discuter", "Pay without arguing"), outcomes: &[("C'est réglé, n'en parlons plus.", "It's settled, let's not talk about it.")], apply: |c| {
                c.life.account *= 0.95;
                0
            } },
            Choice { label: ("Contester avec un avocat", "Contest it with a lawyer"), outcomes: &[("Gagné : tu ne devais rien.", "Won: you owed nothing."), ("Perdu, et l'avocat coûte cher.", "Lost, and the lawyer is expensive.")], apply: |c| {
                if c.chance(0.5) {
                    0
                } else {
                    c.life.account *= 0.9;
                    c.stress_by(6.0);
                    1
                }
            } },
        ],
    },
];

/// The life's news, by their code (`News.n`): a home, a car, a wedding, a birth, a holiday, a luxury,
/// a burn-out, a child grown up, a split, a meeting.
pub fn life_news(n: f64, m: f64, who: &str, fr: bool) -> String {
    let t = |x: Tx| super::tx(x, fr);
    match n as u32 {
        0 => format!("{} {}.", t(("Tu emménages :", "You move in:")), t(HOMES[(m as usize).min(HOMES.len() - 1)].name).to_lowercase()),
        1 => format!("{} {}.", t(("Nouveau moyen de transport :", "New ride:")), t(CARS[(m as usize).min(CARS.len() - 1)].name).to_lowercase()),
        2 => if fr { format!("Vous vous êtes dit oui, {who} et toi !") } else { format!("You and {who} said yes!") },
        3 => if fr { format!("Bienvenue à {who}, ton enfant !") } else { format!("Welcome {who}, your child!") },
        4 => format!("{} {}.", t(("Départ en vacances :", "Off on holiday:")), t(HOLIDAYS[(m as usize).min(HOLIDAYS.len() - 1)].0).to_lowercase()),
        5 => format!("{} {}.", t(("Tu t'offres", "You treat yourself to")), t(LUXURIES[(m as usize).min(LUXURIES.len() - 1)].name).to_lowercase()),
        6 => t(("Burn-out. Le médecin t'arrête : vingt minutes sans travailler.", "Burn-out. The doctor signs you off: twenty minutes without work.")).to_owned(),
        7 => if fr { format!("{who} a grandi et frappe à la porte de ta boîte (voir les candidats).") } else { format!("{who} has grown up and knocks at your company's door (see the candidates).") },
        8 => if m > 0.0 {
            if fr { format!("{who} et toi, c'est fini. Le divorce te coûte la moitié de tout.") } else { format!("You and {who} are over. The divorce costs you half of everything.") }
        } else if fr {
            format!("{who} et toi, c'est fini.")
        } else {
            format!("You and {who} are over.")
        },
        _ => if fr { format!("Tu as rencontré {who}.") } else { format!("You met {who}.") },
    }
}
