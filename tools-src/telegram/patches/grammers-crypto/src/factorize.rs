// Copyright 2020 - developers of the `grammers` project.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

/// A semiprime (product of two distinct primes) to be factorised.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemiPrime(pub u64);

/// The two prime factors of a semiprime, with `p <= q`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrimeFactors {
    pub p: u64,
    pub q: u64,
}

impl PrimeFactors {
    fn new(a: u64, b: u64) -> Self {
        Self {
            p: a.min(b),
            q: a.max(b),
        }
    }
}

fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        let (na, nb) = (b, a % b);
        a = na;
        b = nb;
    }
    a
}

fn modpow(mut n: u128, mut e: u128, m: u128) -> u128 {
    if m == 1 {
        return 0;
    }

    let mut result = 1;
    n %= m;
    while e > 0 {
        if e % 2 == 1 {
            result = (result * n) % m;
        }
        e >>= 1;
        n = (n * n) % m;
    }
    result
}

fn abs_sub(a: u128, b: u128) -> u128 {
    a.max(b) - a.min(b)
}

/// Factorize the given number into its two prime factors.
///
/// The algorithm here is a faster variant of [Pollard's rho algorithm],
/// published by [Richard Brent], based on
/// <https://comeoncodeon.wordpress.com/2010/09/18/pollard-rho-brent-integer-factorization/>.
///
/// Pollard's rho algorithm: <https://en.wikipedia.org/wiki/Pollard%27s_rho_algorithm>
/// Richard Brent: <https://maths-people.anu.edu.au/~brent/pd/rpb051i.pdf>
#[allow(clippy::many_single_char_names)]
pub fn factorize(pq: SemiPrime) -> PrimeFactors {
    const ATTEMPTS: [u64; 5] = [43, 47, 53, 59, 61];
    for attempt in ATTEMPTS {
        // > Note that this algorithm may not find the factors and will return failure for composite n.
        // > In that case, use a different f(x) and try again [...] We choose f(x) = x*x + c
        // Thus by choosing a different `c` we're changing `f(x)` and can try again.
        // Prime factors are used for the attempts in the hopes they'll be more likely to work.
        let c = attempt * (pq.0 / 103);
        let factors = factorize_with_param(pq, c);
        if factors.p != 1 {
            return factors;
        }
    }
    panic!("failed to factorize in a fixed amount of attempts")
}

struct BrentFactorizer {
    pq: u128,
    c: u128,
    m: u128,
    y: u128,
    g: u128,
    r: u128,
    q: u128,
    x: u128,
    ys: u128,
}

impl BrentFactorizer {
    fn new(pq: u128, c: u128) -> Self {
        Self {
            pq,
            c,
            // Random values in the range of 1..pq, chosen by fair dice roll.
            // c is an input free to change in case the chosen value fails.
            y: 3 * (pq / 7),
            m: 7 * (pq / 13),
            g: 1,
            r: 1,
            q: 1,
            x: 0,
            ys: 0,
        }
    }

    fn next_value(&self, value: u128) -> u128 {
        (modpow(value, 2, self.pq) + self.c) % self.pq
    }

    fn advance_power_window(&mut self) {
        self.x = self.y;
        for _ in 0..self.r {
            self.y = self.next_value(self.y);
        }
    }

    fn accumulate_batch(&mut self, steps: u128) {
        for _ in 0..steps {
            self.y = self.next_value(self.y);
            self.q = (self.q * abs_sub(self.x, self.y)) % self.pq;
        }
    }

    fn search_current_window(&mut self) {
        let mut k = 0;

        while k < self.r && self.g == 1 {
            self.ys = self.y;
            self.accumulate_batch(self.m.min(self.r - k));

            self.g = gcd(self.q, self.pq);
            k += self.m;
        }

        self.r *= 2;
    }

    fn recover_factor_after_degenerate_cycle(&mut self) {
        if self.g != self.pq {
            return;
        }

        loop {
            self.ys = self.next_value(self.ys);
            self.g = gcd(abs_sub(self.x, self.ys), self.pq);

            if self.g > 1 {
                return;
            }
        }
    }

    fn factors(&self) -> PrimeFactors {
        let (p, q) = (self.g as u64, (self.pq / self.g) as u64);
        PrimeFactors::new(p, q)
    }
}

fn factorize_with_param(pq: SemiPrime, c: u64) -> PrimeFactors {
    if pq.0 % 2 == 0 {
        return PrimeFactors::new(2, pq.0 / 2);
    }

    let mut factorizer = BrentFactorizer::new(pq.0 as u128, c as u128);

    while factorizer.g == 1 {
        factorizer.advance_power_window();
        factorizer.search_current_window();
    }

    factorizer.recover_factor_after_degenerate_cycle();
    factorizer.factors()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_factorization_1() {
        let pq = factorize(SemiPrime(1470626929934143021));
        assert_eq!((pq.p, pq.q), (1206429347, 1218991343));
    }

    #[test]
    fn test_factorization_2() {
        let pq = factorize(SemiPrime(2363612107535801713));
        assert_eq!((pq.p, pq.q), (1518968219, 1556064227));
    }

    #[test]
    fn test_factorization_3() {
        let pq = factorize(SemiPrime(2804275833720261793));
        assert_eq!((pq.p, pq.q), (1555252417, 1803100129));
    }
}
