use std::io::Cursor;

use age::{Identity, IdentityFile, Recipient};

fn first_data_line(hydrated: &str) -> &str {
    hydrated
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'))
        .unwrap_or("")
}

// Go age 1.3 native keys (`age1pq1`, `AGE-SECRET-KEY-PQ-1`). rage 0.12 only
// has tagpq encrypt. str4d/rage#632 (draft, 0.13) adds pq recipient + identity.
fn reject_unsupported(hydrated: &str) -> Result<(), String> {
    let line = first_data_line(hydrated);
    if line.to_ascii_uppercase().starts_with("AGE-PLUGIN-") {
        return Err("hydrated value is another AGE-PLUGIN identity".into());
    }
    let lower = line.to_ascii_lowercase();
    if lower.starts_with("age-secret-key-pq-") || lower.starts_with("age1pq1") {
        return Err(
            "the age crate cannot use Go age 1.3 post-quantum keys (AGE-SECRET-KEY-PQ / age1pq1)"
                .into(),
        );
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
    reject_unsupported(hydrated)?;
    if let Ok(file) = IdentityFile::from_buffer(Cursor::new(hydrated.as_bytes()))
        && let Ok(rs) = file.to_recipients()
        && !rs.is_empty()
    {
        return Ok(rs);
    }
    if let Ok(r) = parse_recipient_line(first_data_line(hydrated)) {
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
    reject_unsupported(hydrated)?;
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

    fn wrap_ok(hydrated: &str) -> String {
        let recipients = as_recipients(hydrated).unwrap();
        let file_key = age_core::format::FileKey::new(Box::new([7u8; 16]));
        let (stanzas, _) = recipients[0].wrap_file_key(&file_key).unwrap();
        stanzas[0].tag.to_ascii_lowercase()
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
        assert_eq!(wrap_ok(TAG_RECIPIENT), "p256tag");
    }

    #[test]
    fn tagpq_recipient_wraps() {
        let recipient: age::tagpq::Recipient = AGE_TAGPQ_TEST.parse().unwrap();
        assert_eq!(wrap_ok(&recipient.to_string()), "mlkem768p256tag");
    }

    #[test]
    fn go_pq_identity_is_rejected() {
        match as_identities(
            "AGE-SECRET-KEY-PQ-1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq",
        ) {
            Err(err) => assert!(err.contains("post-quantum")),
            Ok(_) => panic!("Go age PQ identity should fail"),
        }
        match as_recipients("age1pq1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq") {
            Err(err) => assert!(err.contains("post-quantum")),
            Ok(_) => panic!("Go age PQ recipient should fail"),
        }
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
    fn comment_header_is_ignored() {
        let id = age::x25519::Identity::generate();
        let blob = format!("# created: now\n{}\n", identity_secret(&id));
        assert!(as_identities(&blob).is_ok());
        assert!(as_recipients(&blob).is_ok());
    }

    // rage 0.12 test vector (`age::tagpq` tests).
    const AGE_TAGPQ_TEST: &str = "age1tagpq1m3e4wvp6hzcrn9exhy0ae3xfx2sjymp594k3tg7j4dpmj922we65vtnmrt2pyallax8669zqkr2pmfchptr4n38kug2xmcmp3adk2lnjqu00x5kxz5pvhmrltvfh9wuq973pcx35cnq8syn9qd3tzpehgztl4xpzr3tpd67g8af9trnjpc05gh7wu536aq4qt2y8zhsm4tvrfpsfl36qs5fpzysnk3sp9w77qzeg49357xex40v4s2lvt620swyys7u8yxdcnu4rkkwxdmt55gsuc3h5c5swahnegjgqwc60hn085ec3sjztwm45l44y3j2at9t6v9zra4ek3kek6waecqm98yaxl37w0d2zra626nz63jdm5sg59w7lyptw83zm6fntd8d0x03a9z6h9prfgpygzar6zrxjcrt4cdctk2mhf95s4a6v4zklfd49xhpsaeujm57thx2x3e3hwzc86ftfhmq5mkxxz3d6r8ws24xj4qfn73eyezg2wy094e3why592pghz27ruq3vkyegrv80eftnw9wqzwgvnwyseaus0yt84fylzrpzp6x2fguxuqjmgudr8xd33qm30evdpxd3jvjg8qh4q60kyq80jgff369k7nrepdc38grd2dava520excqp0ey0x39khx8ry03yffcatgv84fsx5j49djpapedsy693zute5xv5g2ewzrlj5se7akvkc4g4vmzhputpq8eyj9wz5dz6qtn7g3cfpd95nahw4ytspan0feyye04dcylv24ege7zkaj004gjwcxqxfqu2quawa83sx452jqjn8t48czp0xspwgnmvjyhttzzy6nhq8xzkdwnvsfefkwva6asrqc93zjn4rly5gnlv93xy3uzmr39szvjnf63426qzyeyvguc4vdcquwgsxgq236afcpqz866ny4tn7ckc0umefj242rt5vtvwqzzrvfev2mpvqcufp9pqvefyv4ftyuhgausfzuaadsczeykmft5wv3frzgrcp9ztr93h478ke4t86spp2uhyjkj73mp9g92ddk2fpv7v3njzsqgwhq3789sqrgkskehn0zjscckhwftyq4vet7vrlx2hs5kd9cwnq6t0djffhh3zquh4j3p0yaj9z2rc9wykg0usqw7983rrgur9jg8rnnqypwcz2lyclnnc705fc5g3an93ps60q6mxqp85u0ewtxdjlqcks84yduft0a0g6e7naew3v9u2d08knarvajn8q3gq9pgxde3s7nx94lus48wwvw2xjm7k82tvylec2393jdsuvch2xpe77w8hpv9nvsxfsrs270njpmfvpmgyk2cffl9tjp3qqcc4dfkf5rme2dg0x7ew8g39www5smm705q5da4eqvnqwrkavtq6xje9ss38hnkglz4eddz8f5qruvqmq2ff9l22gwkv8h432rdkysy0grkul8e2fedvkyyapfxt760udcgu92m54wl9yavmj4ga3ph9r5n99cjrq6wj5v33x33fe5vkjvfwnnt40wuv2hyexc9f4ylyqv9ldqq9epd4yuv8vrsfx2qy2kqz08kqhnzspy6s0x8fa5c2xkg5y2q0rvz4vnk7rp0acg6eksc3t7cxnn8y7glkjsqja3p56uz6vvhcw55d3ysad0hvsqxpjnc7svenf2gc5xn5kyr0et2vvyruxlnpqcdpqh9pzplumy5yzjxftyzh9ujfw0jq7ee60zx2x23p0jzyh9dvmly8p9h9ysptlqu7kwnejd65dnr75a0np2fvke8xen38r57w6z3wz3mycjmmn267wwxndfh9jdps7uxtct2wwfgamkpa5ap8s96lhfjztpwcm6fguhphu38yunu2v4vz3syzrvgwtqpemkewzp766nyu6texxvjlaemnhyyqutkcy6a42vqfsz49rw5wr4gt70r4vdaasehqjg46fnyts4sthrxadfllha3avu49wsj2c4jx";
}
