use rayon::prelude::*;
use ureq::get;
use scraper::{Html, Selector};
use std::error::Error;
use std::fs::File;
use std::io::{Read, BufRead, BufReader};
use std::collections::HashMap;
use std::env;
use serde::Deserialize;
use regex::Regex;
use std::path::Path;
use std::time::Duration;
use std::thread::sleep;

#[derive(Deserialize)]
struct Package {
    name: String,
    upstream: String,
    selector: Option<String>,
}

fn extract_version(text: &str, pkg_str: &str) -> Result<String, String> {
    let version_pattern = Regex::new(r"\d+\.\d+\.\d+\.\d+|\d+\.\d+\.\d+|\d+\.\d+|\d+").map_err(|e| e.to_string())?;

    let mut vers = text.replace(pkg_str, "")
        .replace("_", "-")
        .to_lowercase();

    // this is likely unnecessary
    if vers.starts_with('v') {
        vers = vers.replacen('v', "", 1);
    }

    match version_pattern.find(&vers) {
        Some(m) => Ok(m.as_str().to_string()),
        _ => Err("Version not found".to_string()),
    }
}

fn determine_default_selector(url: &str) -> Option<&str> {
    let mut selectors = HashMap::new();
    selectors.insert(r"(?i).*github\.com.+\/tags", "div.Box-row:nth-child(1) > div:nth-child(1) > div:nth-child(1) > div:nth-child(1) > div:nth-child(1) > h2:nth-child(1) > a:nth-child(1)");
    selectors.insert(r"(?i).*github\.com.+\/releases\/latest", ".css-truncate > span:nth-child(2)");

    selectors.insert(r"(?i).*gitlab.+\/-\/tags", "li.gl-justify-between:nth-child(1) > div:nth-child(1) > a:nth-child(2)");

    selectors.insert(r"(?i).*pypi\.org.+", ".package-header__name");

    selectors.insert(r"(?i).*download\.savannah\..*gnu.org\/releases.+\/\?C=M&O=D", "tr.e:nth-child(2) > td:nth-child(1) > a:nth-child(1)");

    selectors.insert(r"(?i).*ftp\.gnu\.org\/.+\/\?C=M;O=D", "body > table:nth-child(2) > tbody:nth-child(1) > tr:nth-child(4) > td:nth-child(2) > a:nth-child(1)");

    selectors.insert(r"(?i).*archlinux\.org\/packages\/.+", "#pkgdetails > h2:nth-child(1)");
    selectors.insert(r"(?i).*repology\.org\/project.*\/information.*", ".version-newest");

    selectors.insert(r"(?i).*sourceforge\.net.+\/files.*", ".sub-label");

    selectors.insert(r"(?i).*freedesktop\.org\/.*releases\/.+\/\?C=M;O=D", "body > table:nth-child(2) > tbody:nth-child(1) > tr:nth-child(4) > td:nth-child(2) > a:nth-child(1)");

    let patterns: Vec<(Regex, &str)> = selectors.iter()
        .filter_map(|(key, selector)| Regex::new(key).ok().map(|regex| (regex, *selector)))
        .collect();

    for (pattern, selector) in patterns.iter() {
        if pattern.is_match(url) {
            return Some(selector);
        }
    }

    None
}

fn latest(pkg: &Package) -> Result<String, Box<dyn Error>> {
    if pkg.upstream.is_empty() {
        return Err(format!("Empty upstream for {}", pkg.name).into());
    }

    let mut attempt = 0;
    const MAX_ATTEMPTS: usize = 7;
    const WAIT_TIME: u64 = 1337; // ms

    while attempt < MAX_ATTEMPTS {
        attempt += 1;

        let response = get(&pkg.upstream)
            .set("User-Agent", "stabs")
            .call();

        match response {
            Ok(r) => {
                let document = Html::parse_document(&r.into_string()?);

                let default_selector = determine_default_selector(&pkg.upstream);
                let selector_str = match pkg.selector.as_deref() {
                    Some(s) if !s.is_empty() => s,
                    _ => default_selector.ok_or("No valid selector found")?,
                };

                let selector = Selector::parse(selector_str).map_err(|_| "Invalid selector pattern")?;

                if let Some(element) = document.select(&selector).next() {
                    if let Some(version_text) = element.text().next() {
                        match extract_version(version_text, &pkg.name) {
                            Ok(version) => {
                                return Ok(version);
                            },
                            Err(e) => {
                                eprintln!("Regex failed: {}", e);
                                println!("Raw version text: {}", version_text);
                            }
                        }
                    }
                }
                eprintln!("\x1b[30;3m({}/{}) Retrying '{}'\x1b[0m", attempt, MAX_ATTEMPTS, pkg.name);
            }
            Err(e) => {
                eprintln!("\x1b[30;3m({}/{}) HTTP error for '{}': {}\x1b[0m", attempt, MAX_ATTEMPTS, pkg.name, e);
            }
        }

        sleep(Duration::from_millis(WAIT_TIME));
    }

    Err("Generic failure".into())
}

fn read_versions(file_path: &Path) -> HashMap<String, String> {
    let mut versions = HashMap::new();
    if let Ok(file) = File::open(file_path) {
        let reader = BufReader::new(file);

        for line in reader.lines().map_while(Result::ok) {
            if let Some(stripped) = line.strip_prefix("export") {
                let parts: Vec<&str> = stripped.split("_version=").collect();
                if parts.len() == 2 {
                    let name = parts[0].trim().to_string();
                    let version = parts[1].trim().trim_matches('"').to_string();
                    versions.insert(name, version);
                }
            }
        }
    }
    versions
}

fn read_json(file_path: &Path) -> Result<Vec<Package>, Box<dyn Error>> {
    let mut file = File::open(file_path)?;
    let mut json_string = String::new();
    file.read_to_string(&mut json_string)?;

    let packages: Vec<Package> = serde_json::from_str(&json_string)?;
    Ok(packages)
}

fn main() -> Result<(), Box<dyn Error>> {
    let json_path_str = env::var("RIDPKGSJSON").unwrap_or_else(|_| "/etc/rid/pkgs.json".to_string());
    let vers_path_str = env::var("RIDPKGSVERS").unwrap_or_else(|_| "/etc/rid/versions".to_string());

    let json_path = Path::new(&json_path_str);
    let vers_path = Path::new(&vers_path_str);

    let packages = read_json(json_path)?;
    let env_versions = read_versions(vers_path);

    packages.par_iter().for_each(|pkg| {
        match latest(pkg) {
            Ok(version) => {

                if let Some(env_version) = env_versions.get(&pkg.name) {
                    if version != *env_version {
                        let displayed_version = format!("\x1b[31;1m{}\x1b[0m", version);
                        println!("{}: {} <-> {}", pkg.name, env_version, displayed_version);
                    } else {
                        println!("{}: {} <-> {}", pkg.name, env_version, version);
                    }
                }
            }
            Err(e) => {
                if !e.to_string().contains("upstream") {
                    eprintln!("\x1b[31;1;3mError for '{}': {}\x1b[0m", pkg.name, e);
                }
            }
        }
    });

    Ok(())
}
