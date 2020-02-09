// Copyright 2020 Allan Saddi <allan@saddi.com>
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::collections::HashMap;
use std::path::PathBuf;

use structopt::StructOpt;
use serde::Deserialize;
use serde_json::Value;
use rusoto_core::Region;
use rusoto_ssm::*;
use rusoto_secretsmanager::*;
use tera;
use anyhow::{Context, Result};

mod output;

#[derive(StructOpt, Debug)]
struct Opt {
    /// AWS region.
    #[structopt(long)]
    region: Option<String>,

    /// Increase verbosity.
    #[structopt(short, long, parse(from_occurrences))]
    verbose: u8,

    /// Do not actually write anything out.
    #[structopt(short="n", long="dry-run")]
    dryrun: bool,

    /// Do not back up overwritten files.
    #[structopt(short="B", long="no-backup")]
    nobackup: bool,

    /// Configuration file
    config: PathBuf,
}

#[derive(Debug, Deserialize)]
struct TemplateSpec {
    src: PathBuf,
    out: PathBuf,
}

#[derive(Debug, Deserialize)]
struct Config {
    region: Option<String>,
    parameter_store_prefixes: Option<Vec<String>>,
    secrets: Option<Vec<String>>,
    templates: Vec<TemplateSpec>,
}

fn trim_prefix<'a>(prefix : &str, s: &'a str) -> &'a str {
    &s[prefix.len()+1..]
}

fn get_parameterstore_properties(region: &Region, prefixes: &[String]) -> Result<HashMap<String, String>> {
    let mut data = HashMap::new();

    let client = SsmClient::new(region.clone());

    for prefix in prefixes {
        let mut next_token: Option<String> = None;

        loop {
            let params = client.get_parameters_by_path(GetParametersByPathRequest {
                path: prefix.clone(),
                next_token,
                ..Default::default()
            }).sync().with_context(|| format!("Failed to retrieve parameter {}", prefix))?;

            if let Some(parameters) = params.parameters {
                for p in &parameters {
                    let name = match &p.name {
                        Some(name) => name,
                        None => continue // No name? Skip
                    };
                    let value = match &p.value {
                        Some(value) => value,
                        None => continue // No value? Skip
                    };
                    data.insert(trim_prefix(prefix, name).to_owned(), value.clone());
                }
            }

            next_token = match params.next_token {
                Some(token) => Some(token),
                None => break
            };
        }
    }

    Ok(data)
}

fn get_secretsmanager_properties(region: &Region, secrets: &[String]) -> Result<HashMap<String, String>> {
    let mut _data = HashMap::new();

    let client = SecretsManagerClient::new(region.clone());

    for secret in secrets {
        let _result = client.get_secret_value(GetSecretValueRequest {
            secret_id: secret.clone(),
            ..Default::default()
        }).sync().with_context(|| format!("Failed to get secret {}", secret))?;

        // Only deal with strings
        match _result.secret_string {
            Some(s) => {
                match serde_json::from_str::<Value>(&s) {
                    Ok(Value::Object(map)) => {
                        for (k,jv) in map {
                            match jv {
                                Value::String(v) => { _data.insert(k, v); }
                                _ => eprintln!("WARNING: Secret {}/{} value not JSON string", secret, k)
                            }
                        }
                    }
                    _ => eprintln!("WARNING: Secret {} value not JSON object", secret)
                }
            }
            None => eprintln!("WARNING: Secret {} value not a string", secret)
        }
    }

    Ok(_data)
}

fn merge_properties(properties: Vec<HashMap<String, String>>) -> HashMap<String, String> {
    let mut merged = HashMap::new();

    for prop in properties {
        for (k,v) in prop {
            merged.insert(k, v);
        }
    }

    merged
}

fn get_properties(region: &Region, param_store_prefixes: &[String], secrets: &[String], verbosity: u8) -> Result<HashMap<String, String>> {
    // Retrieve from Parameter Store
    let ps_data = get_parameterstore_properties(region, param_store_prefixes)?;
    if verbosity > 1 { println!("ps_data = {:#?}", ps_data); }

    // Retrieve from Secrets Manager
    let sm_data = get_secretsmanager_properties(region, secrets)?;
    if verbosity > 1 { println!("sm_data = {:#?}", sm_data); }

    // Merge results (Secrets Manager takes precedence)
    let data = merge_properties(vec![ps_data, sm_data]);
    if verbosity > 0 { println!("data = {:#?}", data); }

    Ok(data)
}

fn main() -> Result<()> {
    // Parse command line args
    let opt = Opt::from_args();

    // Parse config file
    let config_bytes = std::fs::read(&opt.config)
        .with_context(|| format!("Error reading config {}", opt.config.display()))?;
    let config: Config = serde_yaml::from_str(&String::from_utf8_lossy(&config_bytes))
        .with_context(|| format!("Error parsing config {}", opt.config.display()))?;

    // Determine region. Priority: command line > config file > environment > profile
    let region = match opt.region {
        Some(region_str) => region_str.parse()?,
        _ => match config.region {
            Some(region_str) => region_str.parse()?,
            _ => Region::default()
        }
    };

    // Retrieve all properties from AWS
    let param_store_prefixes = config.parameter_store_prefixes.unwrap_or_default();
    let secrets = config.secrets.unwrap_or_default();
    let mut context = tera::Context::new();
    for (k,v) in get_properties(&region, &param_store_prefixes, &secrets, opt.verbose)? {
        // Stuff key/value into template context
        context.insert(k, &v);
    }

    // Base directory of config file (for relative templates)
    let mut config_dir = opt.config.clone().canonicalize().unwrap();
    config_dir.pop();

    // Render the templates
    for ts in &config.templates {
        // Determine template path
        let template_path = if ts.src.is_relative() {
            // Relative to config base dir
            let mut base = config_dir.clone();
            base.push(&ts.src);
            base
        } else {
            // Absolute path
            ts.src.clone()
        };

        if opt.verbose > 0 { println!("Rendering template {}...", template_path.display()); }

        let template_bytes = std::fs::read(&template_path)
            .with_context(|| format!("Error reading template {}", template_path.display()))?;
        let template_data: String = String::from_utf8_lossy(&template_bytes).parse()
            .with_context(|| format!("Error parsing template {}", template_path.display()))?;

        let result = tera::Tera::one_off(&template_data, &context, false)
            .with_context(|| format!("Error rendering template {}", template_path.display()))?;

        if !opt.dryrun {
            output::output(&ts.out, result.as_bytes(), opt.nobackup, opt.verbose)?;
        }
    }

    Ok(())
}
