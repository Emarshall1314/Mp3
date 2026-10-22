//! Android's pickers return `content://` URIs. With file access granted, documents on the phone's
//! storage map to file paths, which is what the core opens.

/// The file path of `uri`, or `uri` itself when it is already a path. An error for content URIs
/// with no file behind them (Recent, Downloads, cloud providers).
pub fn local_path(uri: &str) -> Result<String, String> {
    if !uri.starts_with("content://") {
        return Ok(uri.to_owned());
    }
    uri.strip_prefix("content://com.android.externalstorage.documents/")
        .and_then(|rest| rest.rsplit("/document/").next())
        .and_then(|id| id.strip_prefix("document/").or(Some(id)))
        .map(percent_decode)
        .and_then(|id| document_path(&id))
        .ok_or_else(|| {
            "That file has no location on the phone's storage. In the picker, open it from the \
             phone's storage rather than Recent or Downloads."
                .to_owned()
        })
}

/// "primary:Music/x.wav" is /storage/emulated/0/Music/x.wav; "1A2B-3C4D:x.wav" (an SD card) is
/// /storage/1A2B-3C4D/x.wav.
fn document_path(id: &str) -> Option<String> {
    let (volume, relative) = id.split_once(':')?;
    let base = if volume.eq_ignore_ascii_case("primary") {
        "/storage/emulated/0".to_owned()
    } else {
        format!("/storage/{volume}")
    };
    let relative = relative.trim_end_matches('/');
    Some(if relative.is_empty() {
        base
    } else {
        format!("{base}/{relative}")
    })
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(b)) => {
                out.push(b);
                i += 3;
            }
            (b, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::local_path;

    #[test]
    fn maps_local_documents_to_paths() {
        assert_eq!(local_path("/home/me/ir.wav").unwrap(), "/home/me/ir.wav");
        assert_eq!(
            local_path(
                "content://com.android.externalstorage.documents/document/primary%3AMusic%2FEQ%2FHD%20600.txt"
            )
            .unwrap(),
            "/storage/emulated/0/Music/EQ/HD 600.txt"
        );
        assert_eq!(
            local_path(
                "content://com.android.externalstorage.documents/tree/1A2B-3C4D%3AIR/document/1A2B-3C4D%3AIR%2Froom.wav"
            )
            .unwrap(),
            "/storage/1A2B-3C4D/IR/room.wav"
        );
        assert!(
            local_path("content://com.android.providers.downloads.documents/document/msf%3A12")
                .is_err()
        );
        assert!(local_path("content://com.android.externalstorage.documents/document/").is_err());
    }
}
