// Copyright (c) 2018-2020 MobileCoin Inc.

//! mobilecoind daemon entry point

use attest::MrSigner;
use common::logger::{create_app_logger, log, o, Logger};
use consensus_enclave_measurement::sigstruct;
use ledger_db::{Ledger, LedgerDB};
use ledger_sync::{LedgerSyncServiceThread, PollingNetworkState, ReqwestTransactionsFetcher};
use mobilecoind::{
    config::Config, database::Database, payments::TransactionsManager, service::Service,
};
use std::{convert::TryFrom, path::Path};
use structopt::StructOpt;

fn main() {
    let config = Config::from_args();

    common::setup_panic_handler();
    let _sentry_guard = common::sentry::init();
    let (logger, _global_logger_guard) = create_app_logger(o!());

    // Create peer manager.
    let peer_manager = config.peers_config.create_peer_manager(
        MrSigner::try_from(&sigstruct().mrsigner()[..])
            .expect("Could not parse validator node MRSIGNER"),
        &logger,
    );

    // Create network state, transactions fetcher and ledger sync.
    let network_state =
        PollingNetworkState::new(config.quorum_set(), peer_manager.clone(), logger.clone());

    let transactions_fetcher =
        ReqwestTransactionsFetcher::new(config.tx_source_urls.clone(), logger.clone())
            .expect("Failed creating ReqwestTransactionsFetcher");

    // Create the ledger_db.
    let ledger_db = create_or_open_ledger_db(&config, &logger, &transactions_fetcher);

    let _ledger_sync_service_thread = LedgerSyncServiceThread::new(
        ledger_db.clone(),
        peer_manager.clone(),
        network_state,
        transactions_fetcher,
        config.poll_interval,
        logger.clone(),
    );

    // Potentially launch API server
    match (&config.mobilecoind_db, &config.service_port) {
        (Some(mobilecoind_db), Some(service_port)) => {
            log::info!(logger, "Launching mobilecoind API services");

            let _ = std::fs::create_dir_all(mobilecoind_db);

            let mobilecoind_db = Database::new(mobilecoind_db, logger.clone())
                .expect("Could not open mobilecoinddb");

            let transactions_manager = TransactionsManager::new(
                ledger_db.clone(),
                mobilecoind_db.clone(),
                peer_manager,
                logger.clone(),
            );

            let _api_server = Service::new(
                ledger_db,
                mobilecoind_db,
                transactions_manager,
                *service_port,
                config.num_workers,
                logger,
            );

            loop {
                std::thread::sleep(config.poll_interval);
            }
        }

        (None, None) => {
            // No mobilecoind service, only ledger syncing.
            loop {
                std::thread::sleep(config.poll_interval);
            }
        }

        _ => {
            panic!(
                "Please provide both --db and --service-port if you want to enable the API server"
            );
        }
    }
}

fn create_or_open_ledger_db(
    config: &Config,
    logger: &Logger,
    transactions_fetcher: &ReqwestTransactionsFetcher,
) -> LedgerDB {
    // Attempt to open the ledger and see if it has anything in it.
    if let Ok(ledger_db) = LedgerDB::open(config.ledger_db.clone()) {
        if let Ok(num_blocks) = ledger_db.num_blocks() {
            if num_blocks > 0 {
                // Successfully opened a ledger that has blocks in it.
                log::info!(
                    logger,
                    "Ledger DB {:?} opened: num_blocks={} num_txos={}",
                    config.ledger_db,
                    num_blocks,
                    ledger_db.num_txos().expect("Failed getting number of txos")
                );
                return ledger_db;
            }
        }
    }

    // Ledger doesn't exist, or is empty. Copy a bootstrapped ledger or try and get it from the network.
    let ledger_db_file = Path::new(&config.ledger_db).join("data.mdb");
    match &config.ledger_db_bootstrap {
        Some(ledger_db_bootstrap) => {
            log::debug!(
                logger,
                "Ledger DB {:?} does not exist, copying from {}",
                config.ledger_db,
                ledger_db_bootstrap
            );

            // Try and create directory in case it doesn't exist. We need it to exist before we
            // can copy the data.mdb file.
            if !Path::new(&config.ledger_db).exists() {
                std::fs::create_dir_all(config.ledger_db.clone())
                    .unwrap_or_else(|_| panic!("Failed creating directory {:?}", config.ledger_db));
            }

            let src = format!("{}/data.mdb", ledger_db_bootstrap);
            std::fs::copy(src.clone(), ledger_db_file.clone()).unwrap_or_else(|_| {
                panic!(
                    "Failed copying ledger from {} into directory {}",
                    src,
                    ledger_db_file.display()
                )
            });
        }
        None => {
            log::info!(
                    logger,
                    "Ledger DB {:?} does not exist, bootstrapping from peer, this may take a few minutes",
                    config.ledger_db
                );
            std::fs::create_dir_all(config.ledger_db.clone()).expect("Could not create ledger dir");
            LedgerDB::create(config.ledger_db.clone()).expect("Could not create ledger_db");
            let (block, transactions) = transactions_fetcher
                .get_origin_block_and_transactions()
                .expect("Failed to download initial transactions");
            let mut db =
                LedgerDB::open(config.ledger_db.clone()).expect("Could not open ledger_db");
            db.append_block(&block, &transactions, None)
                .expect("Failed to appened initial transactions");
            log::info!(logger, "Bootstrapping completed!");
        }
    }

    // Open ledger and verify it has (at least) the origin block.
    log::debug!(logger, "Opening Ledger DB {:?}", config.ledger_db);
    let ledger_db = LedgerDB::open(config.ledger_db.clone())
        .unwrap_or_else(|_| panic!("Could not open ledger db inside {:?}", config.ledger_db));

    let num_blocks = ledger_db
        .num_blocks()
        .expect("Failed getting number of blocks");
    if num_blocks == 0 {
        panic!("Ledger DB is empty :(");
    }

    log::info!(
        logger,
        "Ledger DB {:?} opened: num_blocks={} num_txos={}",
        config.ledger_db,
        num_blocks,
        ledger_db.num_txos().expect("Failed getting number of txos")
    );

    ledger_db
}
