use regex::Regex;
use urlencoding::decode;

/// 检查URL是否为百度PCS链接
fn is_baidupcs_link(url: &str) -> bool {
    let re = Regex::new(r"^https?://.+\.baidupcs\.com/file/.+$").unwrap();
    re.is_match(url)
}

/// 从百度PCS链接中提取文件名
fn get_file_name_from_baidupcs(url: &str) -> Option<String> {
    // 创建正则表达式匹配 &fin= 参数
    let re = Regex::new(r"&fin=([^&]+)").unwrap();

    if let Some(caps) = re.captures(url) {
        if let Some(encoded_name) = caps.get(1) {
            let encoded_str = encoded_name.as_str();
            // 先处理+符号，将其替换为%20，然后再进行URL解码
            let fixed_encoded = encoded_str.replace('+', "%20");
            if let Ok(decoded) = decode(&fixed_encoded) {
                return Some(decoded.to_string());
            } else {
                // 如果解码失败，尝试直接解码原始字符串
                if let Ok(decoded) = decode(encoded_str) {
                    return Some(decoded.to_string());
                }
            }
        }
    }
    None
}

fn get_file_name_from_pathname(url: &str) -> Option<String> {
    let re = Regex::new(r"\/([^\/?]+)(\?.*)?$").unwrap();
    match re.captures(url) {
        Some(caps) => {
            if let Some(name) = caps.get(1) {
                return Some(name.as_str().to_string());
            } else {
                return None;
            }
        }
        _ => return None,
    }
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
