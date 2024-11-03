use rayon::prelude::*;
use ureq::get;
use scraper::{Html, Selector};
use std::error::Error;
use std::fs::File;
use std::io::{Read, BufRead, BufReader};
use std::collections::HashMap;
use std::env;
use serde::Deserialize;

#[derive(Deserialize)]
struct Package {
    name: String,
    upstream: String,
    selector: Option<String>,
}

fn determine_default_selector(url: &str) -> Option<&str> {
    let mut selectors = HashMap::new();

    selectors.insert("github.com", "div.Box-row:nth-child(1) > div:nth-child(1) > div:nth-child(1) > div:nth-child(1) > div:nth-child(1) > h2:nth-child(1) > a:nth-child(1)");
    selectors.insert("/releases/latest", ".css-truncate > span:nth-child(2)"); // github latest

    selectors.insert("pypi", ".package-header__name");

    selectors.insert("savannah", ".list > tbody:nth-child(1) > tr:nth-child(15) > td:nth-child(1) > a:nth-child(1)");
    selectors.insert("/?C=M;O=D", "body > table:nth-child(2) > tbody:nth-child(1) > tr:nth-child(4) > td:nth-child(2) > a:nth-child(1)"); // ftp.gnu.org sorted by last modified

    selectors.insert("archlinux.org/packages", "#pkgdetails > h2:nth-child(1)");

    for (key, selector) in selectors.iter() {
        if url.contains(key) {
            return Some(selector);
        }
    }

    None
}

fn latest(pkg: &Package) -> Result<String, Box<dyn Error>> {

    if pkg.upstream.is_empty() {
        return Err(format!("Empty upstream for {}", pkg.name).into())
    }

    let response = get(&pkg.upstream).set("User-Agent", "stabs").call()?.into_string()?;
    let document = Html::parse_document(&response);

    let selector: Selector;
    let default_selector = determine_default_selector(&pkg.upstream);

    if let Some(selector_str) = pkg.selector.as_deref().or(default_selector) {
        selector = Selector::parse(selector_str).unwrap();
    } else {
        return Err("No valid selector found".into());
    }


    if let Some(element) = document.select(&selector).next() {
        if let Some(version) = element.text().next() {
            let mut vers = version.trim().to_string().to_lowercase();

            // version string cleanup
            if vers.contains(&pkg.name) {
                vers = vers.replace(&pkg.name, "").trim().to_string();
            }

            if vers.contains('-') {
                let alt_name = pkg.name.replace('_', "-");
                if vers.contains(&alt_name) {
                    vers = vers.replace(&alt_name, "").trim().to_string()
                }
            }

            if vers.starts_with('v') {
                vers = vers.replacen('v', "", 1);
            }

            if vers.starts_with('-') {
                vers = vers.replacen('-', "", 1);
            }

            if let Some(first_part) = vers.split(".t").next() {
                vers = first_part.to_string();
            }

            if let Some(first_part) = vers.split(".zip").next() {
                vers = first_part.to_string();
            }

            if let Some(first_part) = vers.split('-').next() {
                vers = first_part.to_string();
            }

            return Ok(vers);
        }
    }

    Err(format!("Failed to find the latest version for {}", pkg.name).into())
}

fn read_versions(file_path: &str) -> HashMap<String, String> {
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

fn read_json(file_path: &str) -> Result<Vec<Package>, Box<dyn Error>> {
    let mut file = File::open(file_path)?;
    let mut json_string = String::new();
    file.read_to_string(&mut json_string)?;

    let packages: Vec<Package> = serde_json::from_str(&json_string)?;
    Ok(packages)
}

fn main() -> Result<(), Box<dyn Error>> {
    let json_path = env::var("STABS_JSON").unwrap_or_else(|_| "/etc/rid/pkgs.json".to_string());
    let packages = read_json(&json_path)?;
    let env_versions = read_versions("/etc/rid/versions");

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
                eprintln!("Error: {}", e);
            }
        }
    });

    Ok(())
}
