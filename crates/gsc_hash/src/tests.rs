//! Known-vector unit tests. Sources of each vector: `docs/hash-families.md`.

use super::*;

fn h(id: &str, s: &str) -> u64 {
    hash_id(id, s).unwrap_or_else(|| panic!("no family {id}"))
}

#[test]
fn ids_are_unique_and_well_formed() {
    let mut ids: Vec<_> = FAMILIES.iter().map(|f| f.id).collect();
    ids.sort_unstable();
    let n = ids.len();
    ids.dedup();
    assert_eq!(ids.len(), n, "duplicate family id");
    for f in FAMILIES {
        assert!(f.id.contains('.'), "{}", f.id);
        assert!(!f.games.is_empty(), "{}", f.id);
        assert!(matches!(f.bits, 32 | 60 | 63 | 64), "{}", f.id);
    }
}

#[test]
fn eleven_bo7_gsc_families() {
    let gsc = [
        "function",
        "var",
        "namespace",
        "event",
        "bundlevar",
        "dvar",
        "xhash",
        "case",
        "assetlit",
        "script",
        "bundlekey",
    ];
    for name in gsc {
        let f = family(&format!("t10.{name}")).unwrap();
        assert!(f.applies_to(Game::Bo7) && f.applies_to(Game::T10));
    }
}

#[test]
fn fnv_reference() {
    let basis = algo::FNV64_BASIS_TA;
    assert_eq!(
        algo::fnv1a64(b"a", basis, algo::FNV64_PRIME),
        0xaf63dc4c8601ec8c
    );
    assert_eq!(algo::fnv1a32(b"a", algo::FNV32_BASIS), 0xe40c292c);
}

// BO3 name CSV pairs: lower-cased, backslashes not folded.
#[test]
fn t7_canon() {
    assert_eq!(h("t7.canon", "fire_rear_katyushas"), 0x0000d764);
    assert_eq!(h("t7.canon", "Fire_Rear_Katyushas"), 0x0000d764);
    assert_eq!(
        h("t7.canon", "custom_fx/fx_boss_robot_laser_impact"),
        0x0000a4f4
    );
    assert_eq!(h("t7.canon", "field truck not spawned in yet"), 0x0000d38e);
}

// BO4 exe script weapon field table, hash beside name (56/56 such pairs match).
#[test]
fn t89_scr_bo4_fields() {
    for (v, name) in [
        (0x4465ef1e, "healtime"),
        (0x9fdd8e46, "clipmodel"),
        (0x36f164f7, "spinUpTime"),
        (0xc179d23d, "worldModel"),
        (0x1a936250, "firstRaiseTime"),
        (0xe01dbb60, "reloadTime"),
    ] {
        assert_eq!(h("t89.scr", name), v, "{name}");
    }
    // Not FNV-1a 32 from the T7 seed, with or without the final multiply.
    let fnv = algo::fnv1a32(b"healtime", algo::T7_SEED);
    assert_ne!(u64::from(fnv), 0x4465ef1e);
    assert_ne!(u64::from(fnv.wrapping_mul(algo::FNV32_PRIME)), 0x4465ef1e);
}

// bo7-gsc-archive/names pairs, at their stored widths.
#[test]
fn bo7_families() {
    let cases: &[(&str, &str, u64)] = &[
        ("t10.function", "0exp1j983", 0x0ec28edb5ed47500),
        ("t10.function", "8jsw9ezmh", 0x05ff27d10c2b08cf),
        ("t10.var", "2427n_wsteam_aib", 0x046fd9a01ee833ac),
        ("t10.namespace", "aar_tracking", 0x0308dc3d72bc9116),
        ("t10.event", "agent_added", 0x0c93e800bef866d2),
        ("t10.bundlevar", "aararmorybackground", 0x08f3512b414949b0),
        ("t10.dvar", "abom_beam_time", 0x0c663c1d1f1b1fab),
        ("t10.xhash", "0_toys", 0x0e48e026baa17249),
        ("t10.case", "192x256", 0x0ad7f4106022290a),
        ("t10.case", "!=", 0x094f05f6bcb87ee3),
        (
            "t10.assetlit",
            "01a::t10_int_decor_vinyl_record_01",
            0x2d7207625dfc5bd8,
        ),
        ("t10.bundlekey", "aararmorybackground", 0xf889e0fc53eb93c5),
    ];
    for &(id, name, v) in cases {
        assert_eq!(h(id, name), v, "{id} {name}");
    }
    // script_<hex>: the name table keys on 60 bits; the family keeps 63.
    let f = family("t10.script").unwrap();
    assert!(f.verify("scripts/anim/animselector.gsc", 0x069e0e48a1796ca6, 60));
    assert!(f.verify("scripts\\anim\\animselector.gsc", 0x069e0e48a1796ca6, 60));
}

#[test]
fn case_is_raw_bytes() {
    assert_ne!(h("t10.case", "ABC"), h("t10.case", "abc"));
    assert_eq!(h("t10.function", "ABC"), h("t10.function", "abc"));
}

#[test]
fn script_string_xxh3() {
    assert_eq!(h("t10.script_string", "end"), 0x6ee7ba8e155a9ca8);
}

// MWIII/BO6/BO7 localize keys use the IW basis.
#[test]
fn iw_localize() {
    let f = family("iw.localize").unwrap();
    assert!(f.verify("menu/back", 0x0c6d515be207b9a5, 60));
    assert!(f.verify("menu/ok", 0x05c33da8f6ce65b0, 60));
    assert!(f.verify("MENU/CANCEL", 0x078c2c5fe58603ac, 60));
    let ta = family("iw8.localize_ta").unwrap();
    assert!(!ta.verify("menu/back", 0x0c6d515be207b9a5, 60));
}

// GreyhoundPackageIndex pairs.
#[test]
fn sab_and_bones() {
    let sab = "amb\\alarms\\car\\dist\\dist_car_alram_00.LN100.pc.snd";
    assert_eq!(h("t6.sab", sab), 0x32a0beaa);
    assert_eq!(h("iw9.bone", "tag_off"), 0x4fe4db49);
}

// Values from an independent Python port of each algorithm's description.
#[test]
fn older_and_documented_families() {
    assert_eq!(h("iw3.r_hash", "mc/mtl_test"), 0xc0c5c4dd);
    assert_eq!(h("iw4.stringtable", "Weapon_AK47"), 0xe8ab9870);
    assert_eq!(h("t5.com_hashstring", "mp_Nuketown"), 0x6654c95c);
    assert_eq!(h("t5.com_hashkey", "classname"), 0x0001c881);
    assert_eq!(h("t10.omnvar", "ui_hud_shake"), 0x95156aae04eaddd8);
    assert_eq!(h("t10.scr_sp", "main"), 0xf1f6cfe6816eb4d5);
    assert_eq!(h("iw9.scr", "main"), 0xc92dd50e8e081414);
    assert_eq!(h("iw.rawfile_ext", ".gsc"), 0x2d9aa1a0);
    let ifs = "Data/Fast_Files/ff_common.ff";
    assert_eq!(h("codol.ifs_name", ifs), 0x01e424d1cf4c446e);
    assert_eq!(h("codol.ifs_name", "abc"), 0xcbc05d12b47025a5);
}

// Published vectors: MPQ's table keys, Jenkins' lookup3 self-test.
#[test]
fn mpq_and_lookup3() {
    assert_eq!(h("codol.mpq_key", "(hash table)"), 0xc3af3770);
    assert_eq!(h("codol.mpq_key", "(block table)"), 0xec83b3a3);
    let s = b"Four score and seven years ago";
    assert_eq!(algo::hashlittle2(s, 0, 0).0, 0x17770551);
    assert_eq!(algo::hashlittle2(s, 1, 0).0, 0xcd628161);
    assert_eq!(algo::hashlittle2(b"", 0, 0), (0xdeadbeef, 0xdeadbeef));
}

#[test]
fn secure_empty_is_zero() {
    assert_eq!(h("t10.function", ""), 0);
}

#[test]
fn canon() {
    assert_eq!(&*Canon::LowerSlash.apply(b"A\\B/c"), b"a/b/c");
    assert_eq!(&*Canon::LowerBackslash.apply(b"A\\B/c"), b"a\\b\\c");
    assert_eq!(&*Canon::Raw.apply(b"A\\B"), b"A\\B");
    assert_eq!(&*Canon::Lower.apply("É".as_bytes()), "É".as_bytes());
}

#[test]
fn fallback() {
    assert_eq!(fallback_widths(63), &[63, 60]);
    let full = 0xfedc_ba98_7654_3210u64;
    let mut t = NameTable::new();
    t.insert(full, 60, "sixty");
    assert_eq!(t.lookup(full, 63), Some(("sixty", 60)));
    t.insert(full, 63, "sixty-three");
    assert_eq!(t.lookup(full, 63), Some(("sixty-three", 63)));
    assert_eq!(t.lookup(full & MASK60, 60), Some(("sixty", 60)));
    let hit = lookup_with_fallback(full, 64, |k, _| (k == full & MASK60).then_some(()));
    assert_eq!(hit, Some(((), 60)));
}

#[test]
fn insert_verified_rejects_mismatch() {
    let f = family("t7.canon").unwrap();
    let mut t = NameTable::new();
    assert!(t.insert_verified(f, 0xd764, 32, "fire_rear_katyushas"));
    assert!(!t.insert_verified(f, 0xd765, 32, "fire_rear_katyushas"));
    assert_eq!(t.len(), 1);
}

#[test]
fn games_and_lookup() {
    assert!(families_for(Game::T7).any(|f| f.id == "t7.canon"));
    assert_eq!(Game::from_id("BO7"), Some(Game::Bo7));
    assert!(family("T8.FNV64").is_some());
    assert!(hash_id("nope", "x").is_none());
}
