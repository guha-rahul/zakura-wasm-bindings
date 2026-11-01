use super::*;

// A funded wallet crosses the activation boundary without replacing its network
// document or its database. Consensus-only tests cannot exercise retained notes.
#[test]
fn funded_wallet_crosses_nu7_builds_and_signs_then_rewinds() {
    use zcash_client_backend::proto::compact_formats::CompactBlock;
    let words=b"abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
    let parameters=std::str::from_utf8(PARAMS).unwrap().replace('}', ",\"Nu7\":40002}").into_bytes();
    let root=std::env::var("WALLET_TEST_ROOT").unwrap();
    let path=format!("{root}/nu7-crossing-{}.db",uuid::Uuid::new_v4());
    let open=||crate::wallet::initialize_path(&path,"zcash-js-network/2",&parameters,&[3;32]).unwrap();
    let g=open();
    let mut fixture=pczt_build::scan_fixture(true,false);
    fixture["import"]["birthday"]["parameters"]=json!(hex::encode(&parameters));
    let imported:Value=serde_json::from_str(&crate::wallet::signer::signer_create_account(g,"account_import_hd",
        &json!({"accountIndex":0,"birthday":fixture["import"]["birthday"]}).to_string(),words.to_vec(),vec![]).unwrap()).unwrap();
    let account=&imported["account"]["id"];
    let token=imported["signerToken"].as_u64().unwrap() as u32;
    let sync=|g,op,input:Value|serde_json::from_str::<Value>(&crate::wallet::sync::sync_call(g,op,&input.to_string()).unwrap()).unwrap();
    let scan=|g,fixture:&Value| {
        let mut revision:Value=serde_json::from_str::<Value>(&crate::wallet::scan::scan_call(g,"scan_plan",&json!({"target":fixture["target"]}).to_string()).unwrap()).unwrap()["revision"].clone();
        for batch in fixture["batches"].as_array().unwrap() {
            let mut input=batch.clone();input["target"]=fixture["target"].clone();input["revision"]=revision;
            revision=serde_json::from_str::<Value>(&crate::wallet::scan::scan_call(g,"scan_ingest_batch",&input.to_string()).unwrap()).unwrap()["revision"].clone();
        }
    };
    let balance=|g|call(g,"account_balance",json!({"accountId":account,"confirmations":{"trusted":1,"untrusted":1,"allowZeroConfirmationShielding":false}})).unwrap()["amounts"].clone();
    scan(g,&fixture);
    let before=balance(g);
    assert_eq!(before["sapling"]["spendable"],"50000");
    let address=call(g,"address_next",json!({"accountId":account,"request":{"format":"transparent"}})).unwrap();
    let propose=|g|serde_json::from_str::<Value>(&crate::wallet::proposal::proposal_call(g,"proposal_create",&json!({
        "accountId":account,"revision":sync(g,"scan_state",json!({}))["revision"],
        "payments":[{"to":address["address"],"amount":"10000"}],
        "policy":{"spendPools":["sapling"],"transparent":"disallow","changePool":"sapling","feeRule":"zip317-standard",
            "confirmations":{"trusted":1,"untrusted":1,"allowZeroConfirmationShielding":false},"expiry":{"kind":"offset","blocks":40},"lockExpiryBlocks":20}
    }).to_string()).unwrap()).unwrap();
    let prior=propose(g);
    assert_eq!(prior["targetHeight"],40001);assert_eq!(prior["branchId"],0x37a5165bu32);
    crate::wallet::payment::payment_call(g,"payment_abandon",&json!({"operationId":prior["operationId"]}).to_string()).unwrap();
    let mut upgraded=fixture.clone();
    let blocks=upgraded["batches"][0]["blocks"].as_array_mut().unwrap();
    let mut last=CompactBlock::decode(&hex::decode(blocks.last().unwrap().as_str().unwrap()).unwrap()[..]).unwrap();
    for height in 40001..=40002 {
        let next=CompactBlock {height,hash:vec![(height-39990) as u8;32],prev_hash:last.hash.clone(),time:last.time+1,chain_metadata:last.chain_metadata.clone(),..Default::default()};
        blocks.push(json!(hex::encode(next.encode_to_vec())));last=next;
    }
    upgraded["target"]=json!({"height":last.height,"hash":hex::encode(last.hash)});
    scan(g,&upgraded);
    assert_eq!(balance(g),before,"rescanning across activation does not duplicate the deposit");
    let plan=propose(g);
    assert_eq!(plan["targetHeight"],40003);assert_eq!(plan["branchId"],0x77190ad9u32);
    assert_eq!(plan["steps"][0]["transactionVersion"],6);
    let artifact:Value=serde_json::from_str(&crate::wallet::pczt_build::pczt_build_call(g,"pczt_build",&json!({
        "operationId":plan["operationId"],"proposalId":plan["proposalId"],"reviewCommitment":plan["reviewCommitment"]}).to_string()).unwrap()).unwrap();
    let bytes=hex::decode(artifact["bytes"].as_str().unwrap()).unwrap();
    let signed=pczt::Pczt::parse(&crate::wallet::signer::signer_authorize(token,&parameters,&[3;32],40003,0x77190ad9,&bytes,4194304).unwrap()).unwrap();
    let mut verifier=pczt::roles::signer::Signer::new(signed.clone()).unwrap();
    pczt::roles::verifier::Verifier::new(signed).with_sapling::<(),_>(|bundle| {
        assert_eq!(bundle.spends().len(),1);
        verifier.apply_sapling_signature(0,bundle.spends()[0].spend_auth_sig().clone().unwrap()).unwrap();Ok(())
    }).unwrap();
    crate::wallet::storage_close(g).unwrap();
    let g=open();
    let retained:Value=serde_json::from_str(&crate::wallet::pczt_build::pczt_build_call(g,"pczt_get_artifact",&json!({"operationId":plan["operationId"]}).to_string()).unwrap()).unwrap();
    assert_eq!(retained,artifact);
    let current=sync(g,"scan_state",json!({}));
    let rewound=sync(g,"scan_rewind",json!({"revision":current["revision"],"requestedPoint":fixture["target"]}));
    assert!(rewound["point"]["height"].as_u64().unwrap()<40002);
    scan(g,&fixture);
    assert_eq!(balance(g)["sapling"]["total"],before["sapling"]["total"]);
    assert_eq!(crate::wallet::signer::signer_authorize(token,&parameters,&[3;32],40001,0x77190ad9,&bytes,4194304).unwrap_err(),"NETWORK_MISMATCH");
    crate::wallet::storage_close(g).unwrap();crate::wallet::signer::signer_release(token).unwrap();
}
