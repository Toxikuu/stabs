// use reqwest::blocking::Client;
use ureq::get;
use scraper::{Html, Selector};
use std::error::Error;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write, BufRead, BufReader};
use std::collections::HashMap;
use serde::Deserialize;

#[derive(Deserialize)]
struct Package {
    name: String,
    url: String,
    selector: Option<String>,
}

fn determine_default_selector(url: &str) -> Option<&str> {
    let mut selectors = HashMap::new();

    selectors.insert("github.com", "div.Box-row:nth-child(1) > div:nth-child(1) > div:nth-child(1) > div:nth-child(1) > div:nth-child(1) > h2:nth-child(1) > a:nth-child(1)");
    selectors.insert("pypi", ".package-header__name");

    // fucking hell gnu refuses to do this shit consistently making scraping it a nightmare
    selectors.insert("savannah", ".list > tbody:nth-child(1) > tr:nth-child(15) > td:nth-child(1) > a:nth-child(1)");
    selectors.insert("/?C=M;O=D", "body > table:nth-child(2) > tbody:nth-child(1) > tr:nth-child(4) > td:nth-child(2) > a:nth-child(1)"); // ftp.gnu.org sorted by last modified
    // unfortunately, some upstreams don't make it easy to parse the latest version, so
    // arch's repo is included for convenience
    selectors.insert("archlinux.org/packages", "#pkgdetails > h2:nth-child(1)");

    for (key, selector) in selectors.iter() {
        if url.contains(key) {
            return Some(selector);
        }
    }

    None
}

fn latest(pkg: &Package) -> Result<String, Box<dyn Error>> {
    // let client = Client::builder()
    //     .user_agent("tabs")
    //     .build()?;
    //
    // let response = client.get(&pkg.url).send()?.text()?;
    let response = get(&pkg.url).set("User-Agent", "tabs").call()?.into_string()?;
    let document = Html::parse_document(&response);

    let selector: Selector;
    let default_selector = determine_default_selector(&pkg.url);

    if let Some(selector_str) = pkg.selector.as_deref().or(default_selector) {
        selector = Selector::parse(selector_str).unwrap();
    } else {
        return Err("No valid selector found".into());
    }


    if let Some(element) = document.select(&selector).next() {
        if let Some(version) = element.text().next() {
            let mut vers = version.trim().to_string().to_lowercase();

            // println!("vers: {}", vers);

            // version string cleanup
            if vers.contains(&pkg.name) {
                vers = vers.replace(&pkg.name, "").trim().to_string();
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

fn snapshot(versions: &HashMap<String, String>) -> Result<(), Box<dyn Error>> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open("versions.txt")?;
    for (name, version) in versions {
        writeln!(file, "{}={}", name, version)?;
    }
    Ok(())
}

fn read_snapshot() -> HashMap<String, String> {
    let mut versions = HashMap::new();
    if let Ok(file) = File::open("versions.txt") {
        let reader = BufReader::new(file);

        for line in reader.lines().map_while(Result::ok) {
            let parts: Vec<&str> = line.split('=').collect();
            if parts.len() == 2 {
                versions.insert(parts[0].to_string(), parts[1].to_string());
            }
        }
    }

    versions
}

fn read_json() -> Result<Vec<Package>, Box<dyn Error>> {
    let mut file = File::open("tabs.json")?;
    let mut json_string = String::new();
    file.read_to_string(&mut json_string)?;

    let packages: Vec<Package> = serde_json::from_str(&json_string)?;
    Ok(packages)
}

fn main() -> Result<(), Box<dyn Error>> {
    let packages = read_json()?;
    let saved_versions = read_snapshot();
    let mut current_versions = HashMap::new();

    for pkg in packages {
        match latest(&pkg) {
            Ok(version) => {

                let change = if let Some(saved_version) = saved_versions.get(&pkg.name) {
                    if saved_version != &version {
                        format!("\x1b[31m{}\x1b[0m -> ", saved_version)
                    } else {
                        String::new()
                    }
                } else {
                    String::from("\x1b[31m(new)\x1b[0m")
                };

                let displayed_version = if change.contains("(new)") {
                    format!("{} {}", version, change)
                } else if change.contains("->") {
                    format!("{} {}", change, version)
                } else {
                    version.clone()
                };

                println!("{:<16} = {}", pkg.name, displayed_version);

                current_versions.insert(pkg.name.clone(), version);
            }
            Err(e) => eprintln!("Error: {}", e)
        }
    }

    snapshot(&current_versions)?;
    Ok(())
}
