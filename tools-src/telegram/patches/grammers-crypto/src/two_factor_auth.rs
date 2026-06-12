// Copyright 2020 - developers of the `grammers` project.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Functions used by Telegram's [Two-Factor Authentication](https://core.telegram.org/api/srp).

use glass_pumpkin::safe_prime;
use hmac::Hmac;
use num_bigint::{BigInt, BigUint, Sign};
use num_traits::ops::euclid::Euclid;
use sha2::Sha512;

// H(data) := sha256(data)
use crate::sha256 as h;

#[derive(Clone, Copy)]
pub struct SrpGenerator(i32);

impl SrpGenerator {
    pub fn new(value: i32) -> Self {
        Self(value)
    }

    fn as_i32(self) -> i32 {
        self.0
    }

    fn as_u8(self) -> u8 {
        self.0 as u8
    }

    fn as_u32(self) -> u32 {
        self.0 as u32
    }
}

#[derive(Clone, Copy)]
pub struct SrpPrimeBytes<'a>(&'a [u8]);

impl<'a> SrpPrimeBytes<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self(bytes)
    }

    fn as_bytes(self) -> &'a [u8] {
        self.0
    }
}

#[derive(Clone, Copy)]
pub struct SaltPair<'a> {
    salt1: &'a [u8],
    salt2: &'a [u8],
}

impl<'a> SaltPair<'a> {
    pub fn new(salt1: &'a [u8], salt2: &'a [u8]) -> Self {
        Self { salt1, salt2 }
    }
}

pub struct PasswordProofParams<'a, P> {
    pub salts: SaltPair<'a>,
    pub prime: SrpPrimeBytes<'a>,
    pub generator: SrpGenerator,
    pub server_public_value: Vec<u8>,
    pub client_private_nonce: Vec<u8>,
    pub password: P,
}

#[derive(Clone, Copy)]
pub struct SrpGroupParams<'a> {
    prime: SrpPrimeBytes<'a>,
    generator: SrpGenerator,
}

impl<'a> SrpGroupParams<'a> {
    pub fn new(prime: SrpPrimeBytes<'a>, generator: SrpGenerator) -> Self {
        Self { prime, generator }
    }
}

#[derive(Clone, Copy)]
struct SaltedHashInput<'a> {
    data: &'a [u8],
    salt: &'a [u8],
}

#[derive(Clone, Copy)]
struct PasswordHashInput<'a> {
    password: &'a [u8],
    salts: SaltPair<'a>,
}

#[derive(Clone, Copy)]
struct HashPair<'a> {
    left: &'a [u8; 32],
    right: &'a [u8; 32],
}

#[derive(Clone, Copy)]
struct PaddedSrpBytes<'a>(&'a [u8]);

/// Prepare the password for sending to telegram for verification.
/// The method returns *M1* and *g_a* parameters that should be sent to Telegram
/// (without the raw password!).
///
/// The algorithm is described in <https://core.telegram.org/api/srp>.
pub fn calculate_2fa<P: AsRef<[u8]>>(params: PasswordProofParams<'_, P>) -> ([u8; 32], [u8; 256]) {
    let PasswordProofParams {
        salts,
        prime,
        generator,
        server_public_value,
        client_private_nonce,
        password,
    } = params;

    let p = prime.as_bytes();
    let password = password.as_ref();

    // Prepare our parameters
    let big_p = BigInt::from_bytes_be(Sign::Plus, p);

    let g_b = pad_to_256(PaddedSrpBytes(&server_public_value));
    let a = pad_to_256(PaddedSrpBytes(&client_private_nonce));

    let g_for_hash = vec![generator.as_u8()];
    let g_for_hash = pad_to_256(PaddedSrpBytes(&g_for_hash));

    let big_g_b = BigInt::from_bytes_be(Sign::Plus, &g_b);

    let big_g = BigInt::from(generator.as_u32());
    let big_a = BigInt::from_bytes_be(Sign::Plus, &a);

    // k := H(p | g)
    let k = h!(&p, &g_for_hash);
    let big_k = BigInt::from_bytes_be(Sign::Plus, &k);

    // g_a := pow(g, a) mod p
    let g_a = big_g.modpow(&big_a, &big_p);
    let g_a = pad_to_256(PaddedSrpBytes(&g_a.to_bytes_be().1));

    // u := H(g_a | g_b)
    let u = h!(&g_a, &g_b);
    let u = BigInt::from_bytes_be(Sign::Plus, &u);

    // x := PH2(password, salt1, salt2)
    let x = ph2(PasswordHashInput { password, salts });
    let x = BigInt::from_bytes_be(Sign::Plus, &x);

    // v := pow(g, x) mod p
    let big_v = big_g.modpow(&x, &big_p);

    // k_v := (k * v) mod p
    let k_v = (big_k * big_v) % &big_p;

    // t := (g_b - k_v) mod p (positive modulo, if the result is negative increment by p)
    let big_t = (big_g_b - k_v).rem_euclid(&big_p);

    // s_a := pow(t, a + u * x) mod p
    let first = u * x;
    let second = big_a + first;
    let big_s_a = big_t.modpow(&second, &big_p);

    // k_a := H(s_a)
    let k_a = h!(&pad_to_256(PaddedSrpBytes(&big_s_a.to_bytes_be().1)));

    // M1 := H(H(p) xor H(g) | H(salt1) | H(salt2) | g_a | g_b | k_a)
    let h_p = h!(&p);
    let h_g = h!(&g_for_hash);

    let p_xor_g = xor(HashPair {
        left: &h_p,
        right: &h_g,
    });

    let m1 = h!(
        &p_xor_g,
        &h!(&salts.salt1),
        &h!(&salts.salt2),
        &g_a,
        &g_b,
        &k_a
    );

    (m1, g_a)
}

/// Validation for parameters required for Two-Factor authentication.
pub fn check_p_and_g(params: SrpGroupParams<'_>) -> bool {
    if !check_p_len(params.prime) {
        return false;
    }

    check_p_prime_and_subgroup(params)
}

fn check_p_prime_and_subgroup(params: SrpGroupParams<'_>) -> bool {
    let p = &BigUint::from_bytes_be(params.prime.as_bytes());
    let g = params.generator.as_i32();

    if !safe_prime::check(p) {
        return false;
    }

    match g {
        2 => p % 8u8 == BigUint::from(7u8),
        3 => p % 3u8 == BigUint::from(2u8),
        4 => true,
        5 => {
            let mod_value = p % 5u8;
            mod_value == BigUint::from(1u8) || mod_value == BigUint::from(4u8)
        }
        6 => {
            let mod_value = p % 24u8;
            mod_value == BigUint::from(19u8) || mod_value == BigUint::from(23u8)
        }
        7 => {
            let mod_value = p % 7u8;
            mod_value == BigUint::from(3u8)
                || mod_value == BigUint::from(5u8)
                || mod_value == BigUint::from(6u8)
        }
        _ => panic!("Unexpected g parameter"),
    }
}

fn check_p_len(p: SrpPrimeBytes<'_>) -> bool {
    p.as_bytes().len() == 256
}

// SH(data, salt) := H(salt | data | salt)
fn sh(input: SaltedHashInput<'_>) -> [u8; 32] {
    h!(input.salt, input.data, input.salt)
}

// PH1(password, salt1, salt2) := SH(SH(password, salt1), salt2)
fn ph1(input: PasswordHashInput<'_>) -> [u8; 32] {
    let hash = sh(SaltedHashInput {
        data: input.password,
        salt: input.salts.salt1,
    });

    sh(SaltedHashInput {
        data: &hash,
        salt: input.salts.salt2,
    })
}

// PH2(password, salt1, salt2)
//                      := SH(pbkdf2(sha512, PH1(password, salt1, salt2), salt1, 100000), salt2)
fn ph2(input: PasswordHashInput<'_>) -> [u8; 32] {
    let hash1 = ph1(input);

    // 512-bit derived key
    let mut dk = [0u8; 64];
    pbkdf2::pbkdf2::<Hmac<Sha512>>(&hash1, input.salts.salt1, 100000, &mut dk).unwrap();

    sh(SaltedHashInput {
        data: &dk,
        salt: input.salts.salt2,
    })
}

fn xor(input: HashPair<'_>) -> [u8; 32] {
    let mut out = [0; 32];
    out.iter_mut().enumerate().for_each(|(i, o)| {
        *o = input.left[i] ^ input.right[i];
    });
    out
}

fn pad_to_256(data: PaddedSrpBytes<'_>) -> [u8; 256] {
    let data = data.0;
    let mut out = [0; 256];
    out[256 - data.len()..].copy_from_slice(data);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_calculations_1() {
        let salt1 = vec![1];
        let salt2 = vec![2];
        let g = 3;
        let p = pad_to_256(PaddedSrpBytes(&[47]));
        let g_b = vec![5];
        let a = vec![6];
        let password = vec![7];

        let (m1, g_a) = calculate_2fa(PasswordProofParams {
            salts: SaltPair::new(&salt1, &salt2),
            prime: SrpPrimeBytes::new(&p),
            generator: SrpGenerator::new(g),
            server_public_value: g_b,
            client_private_nonce: a,
            password,
        });

        let expected_m1 = vec![
            157, 131, 196, 103, 0, 184, 116, 232, 7, 196, 85, 231, 17, 36, 30, 222, 158, 234, 98,
            88, 59, 56, 71, 215, 183, 123, 122, 50, 19, 32, 54, 206,
        ];
        let expected_g_a = vec![
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 24,
        ];

        assert_eq!(expected_m1, m1);
        assert_eq!(expected_g_a, g_a);
    }

    #[test]
    fn check_calculations_2() {
        let salt1 = decode_hex(
            "5f483c38bd0986e7cdc95ae138ef4f49b951c1f81c713fecdef3af692cec4b4716ac9b770a195ebe",
        );
        let salt2 = decode_hex("b616fc6bbedf511119c5ed34629527f1");
        let g = 3;
        let p = decode_hex("c71caeb9c6b1c9048e6c522f70f13f73980d40238e3e21c14934d037563d930f48198a0aa7c14058229493d22530f4dbfa336f6e0ac925139543aed44cce7c3720fd51f69458705ac68cd4fe6b6b13abdc9746512969328454f18faf8c595f642477fe96bb2a941d5bcd1d4ac8cc49880708fa9b378e3c4f3a9060bee67cf9a4a4a695811051907e162753b56b0f6b410dba74d8a84b2a14b3144e0ef1284754fd17ed950d5965b4b9dd46582db1178d169c6bc465b0d6ff9ca3928fef5b9ae4e418fc15e83ebea0f87fa9ff5eed70050ded2849f47bf959d956850ce929851f0d8115f635b105ee2e4e15d04b2454bf6f4fadf034b10403119cd8e3b92fcc5b");
        let g_b = decode_hex("93f70ebd50f6426aca2568769599f91f24d01284cc51a449e62dcc1527dfe50126b2aa44238e4fb2331419ed4aebf1a0ae15e03abd18bfc52ca6baec564c13b5a1d2e357799897757bb736fdc2ceb1b56aacf19ab3548d6d922a522f0b51f40124c3bc9936aff3e1dcfbea39ac9ad2addc6af0ad30783278bbb84cab0ed8464b0ffeb2b0c93a39a5d97dba0105672ca5475373d8d23e54a6ac9bed9519e8bef4f00719f5ad56151be5537648df2f8e3e7265cb57fb94a054ce2a82b8cc664ad060e0d6c6df1879345440eb977fa0f2d36f31a253d8917732f133d40033a34b6152969b600d59cdabfea2ab2393ad659e56d66e13605b1f61e48e3cd65c0f58ac");
        let a = decode_hex("bf31574f34fce1e53891c59b7f62468a0ca682da8585df8de0a18873359755fbb1818878a9ee919bb1e94d20c5f0607e02a33176199bf3220256c9ea1a69f395a515d20539d88cda750a5252fb864f573f2b032f3b467d08b34fd9c89d1c5d06278e113e51d4e893c1c027455af4653f096607a4156d94fb8e1dc7cee5bfe32850982f941ae4414e83bf22df56270b43b7ccc44c26d40886464da8e344a8540795b8f69b8f508552a723cd6931e1d69204e8e9dc056f0a2a10a0d7951e355f3edef5a5e18a9091295a51eb9db10b8b0d30489c8d29bc0cd86e97781f5e30c5b6bfe7caf4aae81b282e653ac48aa1a8fde789722bc04f4320cd9f86849fe05ca4");
        let password = b"234567".to_vec();

        let (m1, g_a) = calculate_2fa(PasswordProofParams {
            salts: SaltPair::new(&salt1, &salt2),
            prime: SrpPrimeBytes::new(&p),
            generator: SrpGenerator::new(g),
            server_public_value: g_b,
            client_private_nonce: a,
            password,
        });

        let expected_m1 =
            decode_hex("4d7af412c5a2e7b15467376bd118b853604e687b31f51c4980c4d7c1876613e3");
        let expected_g_a = decode_hex("0fa12bc655b1187a28296a69ae565d682782e0ceb05a089c0c41c1dce983dc7f4a53434ea78e065d9e1cb60e427b4468a4780609feba2f55654ee2efe0aeb72edafde2652e26ed5b4d4baad9d2a381806af63416bf6263df45a43d85be5401bc223ebfac09421caddd7e260bd6b86542133c048d6cd54b38d8e2ccdf6b550e875b1353a4acfe3292ffb56a0f58b2a39027c9bfdd91fd4c531d23c77d6e8f7d583eaeea316dedded69935296ce7ea34e9be6ef2fbd829eac4c9bd646dc1563e47f77b91431ca002cf79fc149d968272835c15ca1c6b2c5e03712a2e1b5d52f5e4faa1c16cb177fafd96a6aa5b4b3f4c649915644b63855cfb38561ff17fedfb8a");

        assert_eq!(expected_m1, m1);
        assert_eq!(expected_g_a, g_a);
    }

    #[test]
    fn test_check_p_and_g() {
        // Not prime
        assert_incorrect_pg(PgFixture::new(4, 0));
        // Bad prime
        assert_incorrect_pg(PgFixture::new(13, 0));

        assert_incorrect_pg(PgFixture::new(11, 2));
        assert_correct_pg(PgFixture::new(23, 2));

        assert_incorrect_pg(PgFixture::new(13, 3));
        assert_correct_pg(PgFixture::new(47, 3));

        assert_correct_pg(PgFixture::new(11, 4));

        assert_incorrect_pg(PgFixture::new(13, 5));
        assert_correct_pg(PgFixture::new(11, 5));
        assert_correct_pg(PgFixture::new(179, 5));

        assert_incorrect_pg(PgFixture::new(13, 6));
        assert_correct_pg(PgFixture::new(383, 6));

        assert_incorrect_pg(PgFixture::new(13, 7));
        assert_correct_pg(PgFixture::new(479, 7));
        assert_correct_pg(PgFixture::new(383, 7));
        assert_correct_pg(PgFixture::new(503, 7));
    }

    #[derive(Clone, Copy)]
    struct PgFixture {
        p: u32,
        g: SrpGenerator,
    }

    impl PgFixture {
        fn new(p: u32, g: i32) -> Self {
            Self {
                p,
                g: SrpGenerator::new(g),
            }
        }
    }

    fn assert_incorrect_pg(fixture: PgFixture) {
        let p = fixture.p.to_be_bytes();
        assert!(!check_p_prime_and_subgroup(SrpGroupParams::new(
            SrpPrimeBytes::new(&p),
            fixture.g,
        )));
    }

    fn assert_correct_pg(fixture: PgFixture) {
        let p = fixture.p.to_be_bytes();
        assert!(check_p_prime_and_subgroup(SrpGroupParams::new(
            SrpPrimeBytes::new(&p),
            fixture.g,
        )));
    }

    fn decode_hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }
}
