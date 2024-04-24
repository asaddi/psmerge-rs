use std::collections::HashMap;
use std::path::PathBuf;
use std::fs::File;

use aws_config::{Region, SdkConfig};
use aws_sdk_secretsmanager::types::error::ResourceNotFoundException;
use clap::Parser;
use serde::Deserialize;
use serde_json::Value;
use handlebars::{Handlebars, no_escape};
use anyhow::{Context, Result};
use tokio::{join, runtime::Runtime};

mod model;
mod output;

#[derive(Parser, Debug)]
struct Opt {
    /// AWS region.
    #[clap(long)]
    region: Option<String>,

    /// Increase verbosity.
    #[clap(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    /// Do not actually write anything out.
    #[clap(short='n', long="dry-run")]
    dryrun: bool,

    /// Do not back up overwritten files.
    #[clap(short='B', long="no-backup")]
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

async fn get_parameterstore_properties(config: &SdkConfig, prefixes: &[String]) -> Result<HashMap<String, String>> {
    let mut data = HashMap::new();

    let client = aws_sdk_ssm::Client::new(config);

    for prefix in prefixes {
        let prefix = prefix.strip_suffix('/').unwrap_or(prefix);
        let prefix_with_slash = {
            let mut s = String::with_capacity(prefix.len() + 1);
            s.push_str(prefix);
            s.push('/');
            s
        };

        let mut next_token: Option<String> = None;

        loop {
            let params = client.get_parameters_by_path()
                .path(&prefix_with_slash)
                .with_decryption(true)
                .set_next_token(next_token) // It's an Option, so use this instead of next_token()
                .send().await.with_context(|| format!("Failed to retrieve parameter {}", prefix))?;
            // let params = client.get_parameters_by_path(GetParametersByPathRequest {
            //     path: prefix_with_slash.clone(),
            //     next_token,
            //     ..Default::default()
            // }).await.with_context(|| format!("Failed to retrieve parameter {}", prefix))?;

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

async fn get_secretsmanager_properties(config: &SdkConfig, secrets: &[String]) -> Result<HashMap<String, String>> {
    let mut data = HashMap::new();

    let client = aws_sdk_secretsmanager::Client::new(config);

    for secret in secrets {
        let result = match client.get_secret_value()
            .secret_id(secret)
            .send().await.with_context(|| format!("Failed to get secret {}", secret)) {
            Ok(response) => response,
            Err(e) => {
                // Ignore if it's ResourceNotFound
                if e.root_cause().downcast_ref::<ResourceNotFoundException>().is_some() {
                    continue;
                }
                // Everything else
                return Err(e);
            }
        };

        // Only deal with strings
        match result.secret_string {
            Some(s) => {
                match serde_json::from_str::<Value>(&s) {
                    Ok(Value::Object(map)) => {
                        for (k,jv) in map {
                            match jv {
                                Value::String(v) => { data.insert(k, v); }
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

    Ok(data)
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

async fn get_properties(config: &SdkConfig, param_store_prefixes: &[String], secrets: &[String], verbosity: u8) -> Result<HashMap<String, String>> {
    // Retrieve from Parameter Store
    let ps_fut = get_parameterstore_properties(config, param_store_prefixes);

    // Retrieve from Secrets Manager
    let sm_fut = get_secretsmanager_properties(config, secrets);

    // TODO Could probably use try_join! here... But how?
    let (ps_res, sm_res) = join!(ps_fut, sm_fut);

    let ps_data = ps_res?;
    let sm_data = sm_res?;

    if verbosity > 1 {
        println!("ps_data = {:#?}", ps_data);
        println!("sm_data = {:#?}", sm_data);
    }

    // Merge results (Secrets Manager takes precedence)
    let data = merge_properties(vec![ps_data, sm_data]);
    if verbosity > 0 { println!("data = {:#?}", data); }

    Ok(data)
}

fn main() -> Result<()> {
    // Parse command line args
    let opt = Opt::parse();

    // Parse config file
    let config_bytes = std::fs::read(&opt.config)
        .with_context(|| format!("Error reading config {}", opt.config.display()))?;
    let config: Config = serde_yaml::from_str(&String::from_utf8_lossy(&config_bytes))
        .with_context(|| format!("Error parsing config {}", opt.config.display()))?;

    // Only need to selectively go async
    let rt = Runtime::new().unwrap();
    let base_sdk_config = rt.block_on(aws_config::load_from_env());

    // Determine region. Priority: command line > config file > environment > profile
    let region = match opt.region {
        Some(region_str) => Some(Region::new(region_str)),
        _ => config.region.map(Region::new)
    };

    // Switch to new region, if needed
    let sdk_config = match region {
        Some(region) => base_sdk_config.into_builder().region(region).build(),
        _ => base_sdk_config
    };

    // Retrieve all properties from AWS
    let param_store_prefixes = config.parameter_store_prefixes.unwrap_or_default();
    let secrets = config.secrets.unwrap_or_default();

    let data = rt.block_on(get_properties(&sdk_config, &param_store_prefixes, &secrets, opt.verbose))?;

    // Generate (JSON) template model
    let model = model::build_template_model(data);
    if opt.verbose > 1 { println!("model = {:#?}", model); }

    // Initialize template engine
    let mut handlebars = Handlebars::new();
    handlebars.register_escape_fn(no_escape);
    handlebars.set_strict_mode(true);

    // Base directory of config file (for relative templates)
    let mut config_dir = opt.config.canonicalize().unwrap();
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

        let mut template_file = File::open(&template_path)
            .with_context(|| format!("Error reading template {}", template_path.display()))?;

        let mut result: Vec<u8> = Vec::new();
        handlebars.render_template_source_to_write(&mut template_file, &model, &mut result)
            .with_context(|| format!("Error rendering template {}", template_path.display()))?;

        if !opt.dryrun {
            output::output(&ts.out, &result, opt.nobackup, opt.verbose)?;
        }
    }

    Ok(())
}
