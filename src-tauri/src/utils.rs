use regex::Regex;
use urlencoding::decode;

lazy_static::lazy_static! {
    static ref BAIDUPCS_URL_REGEX: Regex = Regex::new(r"^https?://.+\.baidupcs\.com/file/.+$").unwrap();
    static ref FIN_PARAM_REGEX: Regex = Regex::new(r"&fin=([^&]+)").unwrap();
    static ref FILENAME_REGEX: Regex = Regex::new(r"\/([^\/?]+)(\?.*)?$").unwrap();
}

/// 检查URL是否为百度PCS链接
pub fn is_baidupcs_link(url: &str) -> bool {
    BAIDUPCS_URL_REGEX.is_match(url)
}

/// 从百度PCS链接中提取文件名
pub fn get_file_name_from_baidupcs(url: &str) -> Option<String> {
    if let Some(caps) = FIN_PARAM_REGEX.captures(url) {
        if let Some(encoded_name) = caps.get(1) {
            let encoded_str = encoded_name.as_str();
            let fixed_encoded = encoded_str.replace('+', "%20");
            if let Ok(decoded) = decode(&fixed_encoded) {
                return Some(decoded.to_string());
            } else {
                if let Ok(decoded) = decode(encoded_str) {
                    return Some(decoded.to_string());
                }
            }
        }
    }
    None
}

pub fn get_file_name_from_pathname(url: &str) -> Option<String> {
    if let Some(caps) = FILENAME_REGEX.captures(url) {
        if let Some(name) = caps.get(1) {
            if let Ok(decoded) = decode(name.as_str()) {
                return Some(decoded.to_string());
            }
        }
    }
    None
}

pub fn get_file_name(url: &str) -> Option<String> {
    // 检查是否为百度PCS链接
    if is_baidupcs_link(url) {
        // 从百度PCS链接中提取文件名
        get_file_name_from_baidupcs(url)
    } else {
        // 使用正则表达式从URL中提取文件名
        get_file_name_from_pathname(url)
    }
}
