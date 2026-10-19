use std::io::Cursor;

use age::{Identity, IdentityFile, Recipient};

use super::pq;

fn first_data_line(hydrated: &str) -> &str {
    hydrated
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'))
        .unwrap_or("")
}

// Any line counts: IdentityFile would run the plugin of a later line too, and
// an AGE-PLUGIN-LADE line would make this plugin call itself.
fn reject_plugin_identity(hydrated: &str) -> Result<(), String> {
    if hydrated
        .lines()
        .any(|l| l.trim().to_ascii_uppercase().starts_with("AGE-PLUGIN-"))
    {
        return Err("hydrated value holds an AGE-PLUGIN identity".into());
    }
    Ok(())
}

fn parse_recipient_line(line: &str) -> Result<Box<dyn Recipient + Send>, String> {
    if let Ok(r) = line.parse::<age::x25519::Recipient>() {
        return Ok(Box::new(r));
    }
    if let Ok(r) = line.parse::<age::tag::Recipient>() {
        return Ok(Box::new(r));
    }
    if let Ok(r) = line.parse::<age::tagpq::Recipient>() {
        return Ok(Box::new(r));
    }
    if let Ok(r) = line.parse::<age::ssh::Recipient>() {
        return Ok(Box::new(r));
    }
    Err("age could not use the hydrated value as a recipient".into())
}

pub(super) fn as_recipients(hydrated: &str) -> Result<Vec<Box<dyn Recipient + Send>>, String> {
    reject_plugin_identity(hydrated)?;
    let line = first_data_line(hydrated);
    if pq::is_secret(line) {
        return pq::recipients(hydrated);
    }
    if pq::is_recipient(line) {
        return Ok(vec![pq::public_recipient(line)?]);
    }
    if let Ok(file) = IdentityFile::from_buffer(Cursor::new(hydrated.as_bytes()))
        && let Ok(rs) = file.to_recipients()
        && !rs.is_empty()
    {
        return Ok(rs);
    }
    if let Ok(r) = parse_recipient_line(line) {
        return Ok(vec![r]);
    }
    if let Ok(id) = age::ssh::Identity::from_buffer(Cursor::new(hydrated.as_bytes()), None) {
        let r = age::ssh::Recipient::try_from(id)
            .map_err(|e| format!("age rejected the hydrated value: {e:?}"))?;
        return Ok(vec![Box::new(r)]);
    }
    Err("age could not use the hydrated value as a recipient".into())
}

pub(super) fn as_identities(
    hydrated: &str,
) -> Result<Vec<Box<dyn Identity + Send + Sync>>, String> {
    reject_plugin_identity(hydrated)?;
    if pq::is_secret(first_data_line(hydrated)) {
        return pq::identities(hydrated);
    }
    if let Ok(file) = IdentityFile::from_buffer(Cursor::new(hydrated.as_bytes())) {
        return file.into_identities().map_err(|e| e.to_string());
    }
    if let Ok(id) = age::ssh::Identity::from_buffer(Cursor::new(hydrated.as_bytes()), None) {
        return Ok(vec![Box::new(id)]);
    }
    Err("age could not use the hydrated value as an identity".into())
}

#[cfg(test)]
mod tests {
    use age_core::secrecy::ExposeSecret;

    use super::*;

    const TAG_RECIPIENT: &str =
        "age1tag1qt8lw0ual6avlwmwatk888yqnmdamm7xfd0wak53ut6elz5c4swx2yqdj4e";

    fn identity_secret(id: &age::x25519::Identity) -> String {
        id.to_string().expose_secret().to_string()
    }

    fn wrap_ok(hydrated: &str) -> (String, std::collections::HashSet<String>) {
        let recipients = as_recipients(hydrated).unwrap();
        let file_key = age_core::format::FileKey::new(Box::new([7u8; 16]));
        let (stanzas, labels) = recipients[0].wrap_file_key(&file_key).unwrap();
        (stanzas[0].tag.to_ascii_lowercase(), labels)
    }

    #[test]
    fn identity_file_wraps_and_unwraps() {
        let id = age::x25519::Identity::generate();
        let secret = identity_secret(&id);
        let file_key = age_core::format::FileKey::new(Box::new([7u8; 16]));
        let recipients = as_recipients(&secret).unwrap();
        let (stanzas, _) = recipients[0].wrap_file_key(&file_key).unwrap();
        assert_eq!(stanzas[0].tag.to_ascii_lowercase(), "x25519");
        let identities = as_identities(&secret).unwrap();
        let got = identities[0].unwrap_stanzas(&stanzas).unwrap().unwrap();
        assert_eq!(got.expose_secret(), file_key.expose_secret());
    }

    #[test]
    fn public_recipient_wraps() {
        let id = age::x25519::Identity::generate();
        let public = id.to_public().to_string();
        let file_key = age_core::format::FileKey::new(Box::new([3u8; 16]));
        let recipients = as_recipients(&public).unwrap();
        let (stanzas, _) = recipients[0].wrap_file_key(&file_key).unwrap();
        let identities = as_identities(&identity_secret(&id)).unwrap();
        let got = identities[0].unwrap_stanzas(&stanzas).unwrap().unwrap();
        assert_eq!(got.expose_secret(), file_key.expose_secret());
    }

    #[test]
    fn tag_recipient_wraps() {
        let (tag, labels) = wrap_ok(TAG_RECIPIENT);
        assert_eq!(tag, "p256tag");
        assert!(labels.is_empty(), "{labels:?}");
    }

    #[test]
    fn tagpq_recipient_wraps() {
        let recipient: age::tagpq::Recipient = AGE_TAGPQ_TEST.parse().unwrap();
        let (tag, labels) = wrap_ok(&recipient.to_string());
        assert_eq!(tag, "mlkem768p256tag");
        assert!(labels.contains("postquantum"), "{labels:?}");
    }

    fn without_pq_plugin<T>(f: impl FnOnce() -> T) -> T {
        let empty = tempfile::tempdir().unwrap();
        temp_env::with_var("PATH", Some(empty.path()), f)
    }

    fn assert_missing_pq_plugin<T>(res: Result<T, String>) {
        match res {
            Err(err) => assert!(err.contains("age-plugin-pq not found on PATH"), "{err}"),
            Ok(_) => panic!("expected a missing age-plugin-pq error"),
        }
    }

    #[test]
    fn pq_secret_without_plugin_names_it() {
        let secret =
            "AGE-SECRET-KEY-PQ-1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq";
        without_pq_plugin(|| {
            assert_missing_pq_plugin(as_identities(secret));
            assert_missing_pq_plugin(as_recipients(secret));
        });
    }

    #[test]
    fn pq_recipient_without_plugin_names_it() {
        without_pq_plugin(|| assert_missing_pq_plugin(as_recipients(AGE_PQ_RECIPIENT)));
    }

    #[test]
    fn oversized_pq_identity_fails_before_spawn() {
        let line =
            "AGE-SECRET-KEY-PQ-1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq\n";
        let blob = line.repeat(80);
        without_pq_plugin(|| match as_identities(&blob) {
            Err(err) => assert!(err.contains("over the 4096 bytes"), "{err}"),
            Ok(_) => panic!("expected an oversized identity error"),
        });
    }

    fn pq_round_trip(recipient: &str, secret: &str) {
        let file_key = age_core::format::FileKey::new(Box::new([5u8; 16]));
        let recipients = as_recipients(recipient).unwrap();
        let (stanzas, labels) = recipients[0].wrap_file_key(&file_key).unwrap();
        assert_eq!(stanzas[0].tag, "mlkem768x25519");
        assert!(labels.contains("postquantum"), "{labels:?}");
        let identities = as_identities(secret).unwrap();
        let got = identities[0].unwrap_stanzas(&stanzas).unwrap().unwrap();
        assert_eq!(got.expose_secret(), file_key.expose_secret());
    }

    #[test]
    fn pq_recipient_round_trip() {
        pq::with_stable_path(|| {
            let (secret, recipient) = pq::test_keygen();
            pq_round_trip(&recipient, &secret);
        });
    }

    #[test]
    fn pq_secret_as_recipient_round_trip() {
        pq::with_stable_path(|| {
            let (secret, _) = pq::test_keygen();
            pq_round_trip(&secret, &secret);
        });
    }

    #[test]
    fn pq_key_file_with_comments_round_trip() {
        pq::with_stable_path(|| {
            let (secret, recipient) = pq::test_keygen();
            let file = format!("# created: now\n# public key: {recipient}\n  {secret}\n");
            pq_round_trip(&file, &file);
        });
    }

    #[test]
    fn plugin_identity_is_rejected() {
        match as_identities("AGE-PLUGIN-OTHER-1qypqxpq9qypqxpq9qypqxpq9qypqxpq9") {
            Err(err) => assert!(err.contains("AGE-PLUGIN")),
            Ok(_) => panic!("plugin identity should fail"),
        }
        match as_recipients("AGE-PLUGIN-OTHER-1qypqxpq9qypqxpq9qypqxpq9qypqxpq9") {
            Err(err) => assert!(err.contains("AGE-PLUGIN")),
            Ok(_) => panic!("plugin recipient should fail"),
        }
    }

    #[test]
    fn plugin_identity_on_a_later_line_is_rejected() {
        let id = age::x25519::Identity::generate();
        let blob = format!(
            "{}\n  AGE-PLUGIN-LADE-1qypqxpq9qypqxpq9qypqxpq9qypqxpq9\n",
            identity_secret(&id)
        );
        match as_identities(&blob) {
            Err(err) => assert!(err.contains("AGE-PLUGIN"), "{err}"),
            Ok(_) => panic!("a later plugin identity should fail"),
        }
        match as_recipients(&blob) {
            Err(err) => assert!(err.contains("AGE-PLUGIN"), "{err}"),
            Ok(_) => panic!("a later plugin identity should fail as a recipient"),
        }
    }

    #[test]
    fn comment_header_is_ignored() {
        let id = age::x25519::Identity::generate();
        let blob = format!("# created: now\n{}\n", identity_secret(&id));
        assert!(as_identities(&blob).is_ok());
        assert!(as_recipients(&blob).is_ok());
    }

    // `age-keygen -pq | age-keygen -y` from Go age 1.3.2. The secret was discarded.
    const AGE_PQ_RECIPIENT: &str = "age1pq1q934kjs3h7uyndnllqwzyjyg0d7vrhn6etvu43lefy46lsjgspr46a6qn82xsptev40flxfhkes5526cdkhsczp9ypv99rpfe72v0r7nphn6j6fm6uppyy70r0mn3y64kkmpgdprz33c5jesw6vhgdg825kys5uxuawxcgan2sxr2wk9jwck0xgwjpznl6vv66x9km3znhdlfjuquultnyn6vu28a9x4netyzxwr67zcmy38lr7fduyppy49y2wlaj2fmeqru4gns7eu4gme4p45g56zm9ulp324lfu93w7gy3wewmp33ecn4q66cxa9n2d8szr9rjnf85r9rlfz9muuqzkjp3nlk7tg2jyqyszpk2drs46e5zragstas92jgq9hv0cgf5a3ydsplqj4xps5455ym0v6jfaxjgea6ppuydv546t3duf5tm65pyd6ggtur9f05mev3kr6xlxcwajferpshykq4f9qht6tvu0hcln0ltpkwdnhydpqpdkgexrk50fcsdc2rx2plxz6wqqgq6wqt8pa4ydjk3x8rsq4ack2qhaf424yvczp5ac99u2rrfykvss8v5d9674tgxefe6pjdhkmv38x2z7skcqqsrx26wn8u38x9m7fz66kagyrgfa6cfkyax0qy20t2zgkdfp7n4zx8fs9xepmyfnhxhtql26ugu9kwk6cxpex88frje24mylvnz9lsxz6z6vpeqjk5vaehfxda0zczva5y94m8fjxwug0s5zste6fp9t52j59vu8ynf5zzy3yv3yqn6xtku5y8qz30pmt43rzxve5hmxq0825wy6hpsdrcfrrvg2zhvaqfllhdrca0xfv5vex3p9saduzgwdmk4p5y8khk59wdd6lp764hum2vpdmvglgwwdyndqlrudswtgvnwtzg0h9c2427sy3yn62qe2x59lcdpxfqgk0kqemv7g5ujhg3dhtfp3f6x0pu69520rcd5expwq29gwqhfxaqkd5w52zq77yvn7lp32jprrphaffq3d6828zeag9ysnuz26hgdcqelnkduu3tmw24gjax72gwjucdggydzyz868qxthw6x6uk9zllysvqkr3w5fe2vckvsalzscm5a6g3upujsmvyuxsytpnjz4uxk98qqstprj58jdu9x5ddyj2hlyh20mpp78yxhd6t9hst9m0z84cj52rgaywetw0tnr4ca4pzaau4s4tr5vrpjhrtz0v674yk7n3d3d9p4a8z3nhdsslcz67ulm56pak0jpn39qnkyy7eu34kw2g2282pmltn2nfcq2ue5t0reet6xts0cclgf32atqmnw98aewxju6p22adxz5nfjaqp8zcqre5ahqpkuckk34pygnjl52epvyuryt2z6vufspacjn6nxnyucmc4ejs2cu3gaumc0zakaev3mc3ep0tga0jkp995env6ae5c5t4hhguxy3h5ws35jltvrq4wzpzsgtz847n8ppf8yd75c58erpf2kzery4rev6zj63gr8yng66zw6nq33tmqke6nyh4fxnzlm7ztak95ke8pp8h7g09pfxpz3aqtz6cfc0756ul0gxyljud6kfkhd3f3lqcdp88gmx4x4suwq3y2shvqplec8uy3gft0l9cd3n2wx6nkapn2hff69yqr9ddpstpyllygwhx0ttczc8lqgewdzmg440y03x2fr6glvyd53gpnrw27zapzm28fvewxw6qyd57d87pm35jpfkz5r7cfvftxyy2enjyhyhg5mzewklxuk429wvz3pzs4rgv9sgu3sxtvkh8suvqcmsj8j3tsj2wwqnl6ztdzpul57cmh3wmrsq65cl7ueats9vz6c2gwhus88jtza52qgr9gkx8r0p3wj9e03npts9xymupc268jf0gt7g4lqwkpgdy70nlhxzvzgp5m76t";

    // rage 0.12 test vector (`age::tagpq` tests).
    const AGE_TAGPQ_TEST: &str = "age1tagpq1m3e4wvp6hzcrn9exhy0ae3xfx2sjymp594k3tg7j4dpmj922we65vtnmrt2pyallax8669zqkr2pmfchptr4n38kug2xmcmp3adk2lnjqu00x5kxz5pvhmrltvfh9wuq973pcx35cnq8syn9qd3tzpehgztl4xpzr3tpd67g8af9trnjpc05gh7wu536aq4qt2y8zhsm4tvrfpsfl36qs5fpzysnk3sp9w77qzeg49357xex40v4s2lvt620swyys7u8yxdcnu4rkkwxdmt55gsuc3h5c5swahnegjgqwc60hn085ec3sjztwm45l44y3j2at9t6v9zra4ek3kek6waecqm98yaxl37w0d2zra626nz63jdm5sg59w7lyptw83zm6fntd8d0x03a9z6h9prfgpygzar6zrxjcrt4cdctk2mhf95s4a6v4zklfd49xhpsaeujm57thx2x3e3hwzc86ftfhmq5mkxxz3d6r8ws24xj4qfn73eyezg2wy094e3why592pghz27ruq3vkyegrv80eftnw9wqzwgvnwyseaus0yt84fylzrpzp6x2fguxuqjmgudr8xd33qm30evdpxd3jvjg8qh4q60kyq80jgff369k7nrepdc38grd2dava520excqp0ey0x39khx8ry03yffcatgv84fsx5j49djpapedsy693zute5xv5g2ewzrlj5se7akvkc4g4vmzhputpq8eyj9wz5dz6qtn7g3cfpd95nahw4ytspan0feyye04dcylv24ege7zkaj004gjwcxqxfqu2quawa83sx452jqjn8t48czp0xspwgnmvjyhttzzy6nhq8xzkdwnvsfefkwva6asrqc93zjn4rly5gnlv93xy3uzmr39szvjnf63426qzyeyvguc4vdcquwgsxgq236afcpqz866ny4tn7ckc0umefj242rt5vtvwqzzrvfev2mpvqcufp9pqvefyv4ftyuhgausfzuaadsczeykmft5wv3frzgrcp9ztr93h478ke4t86spp2uhyjkj73mp9g92ddk2fpv7v3njzsqgwhq3789sqrgkskehn0zjscckhwftyq4vet7vrlx2hs5kd9cwnq6t0djffhh3zquh4j3p0yaj9z2rc9wykg0usqw7983rrgur9jg8rnnqypwcz2lyclnnc705fc5g3an93ps60q6mxqp85u0ewtxdjlqcks84yduft0a0g6e7naew3v9u2d08knarvajn8q3gq9pgxde3s7nx94lus48wwvw2xjm7k82tvylec2393jdsuvch2xpe77w8hpv9nvsxfsrs270njpmfvpmgyk2cffl9tjp3qqcc4dfkf5rme2dg0x7ew8g39www5smm705q5da4eqvnqwrkavtq6xje9ss38hnkglz4eddz8f5qruvqmq2ff9l22gwkv8h432rdkysy0grkul8e2fedvkyyapfxt760udcgu92m54wl9yavmj4ga3ph9r5n99cjrq6wj5v33x33fe5vkjvfwnnt40wuv2hyexc9f4ylyqv9ldqq9epd4yuv8vrsfx2qy2kqz08kqhnzspy6s0x8fa5c2xkg5y2q0rvz4vnk7rp0acg6eksc3t7cxnn8y7glkjsqja3p56uz6vvhcw55d3ysad0hvsqxpjnc7svenf2gc5xn5kyr0et2vvyruxlnpqcdpqh9pzplumy5yzjxftyzh9ujfw0jq7ee60zx2x23p0jzyh9dvmly8p9h9ysptlqu7kwnejd65dnr75a0np2fvke8xen38r57w6z3wz3mycjmmn267wwxndfh9jdps7uxtct2wwfgamkpa5ap8s96lhfjztpwcm6fguhphu38yunu2v4vz3syzrvgwtqpemkewzp766nyu6texxvjlaemnhyyqutkcy6a42vqfsz49rw5wr4gt70r4vdaasehqjg46fnyts4sthrxadfllha3avu49wsj2c4jx";
}
