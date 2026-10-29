use super::*;
use crate::governance::test_support as fixture;
fn setup() -> Database { let db=fixture::database(); let p=fixture::provider("http://127.0.0.1:1"); db.save_provider(&p).unwrap(); db.set_current_provider(&p.id).unwrap(); fixture::budget(&db); db }
fn count(db:&Database)->i64 { let c=db.conn(); let c=c.lock().unwrap(); c.query_row("SELECT count(*) FROM config_versions",[],|r|r.get(0)).unwrap() }
#[test]
fn preview_is_read_only_and_stale_current_or_target_is_rejected() {
    let db=setup(); let v=capture(&db,"known good").unwrap(); let p=preview(&db,&v.id).unwrap(); assert_eq!(count(&db),1);
    let mut provider=db.get_provider("gov-test").unwrap().unwrap(); provider.name="modified".into(); db.save_provider(&provider).unwrap();
    assert!(restore(&db,&v.id,&p.expected_current_hash,&p.target_hash).unwrap_err().contains("changed after preview")); assert_eq!(count(&db),1);
    let p=preview(&db,&v.id).unwrap();
    { let c=db.conn(); let c=c.lock().unwrap(); c.execute("UPDATE config_versions SET content='{}' WHERE id=?1",[&v.id]).unwrap(); }
    assert!(restore(&db,&v.id,&p.expected_current_hash,&p.target_hash).is_err()); assert_eq!(db.get_provider("gov-test").unwrap().unwrap().name,"modified");
}
#[test]
fn restoration_keeps_new_providers_and_safety_settings_and_makes_backup() {
    let db=setup(); db.set_setting("proxy_settings",r#"{"enabled":true,"host":"127.0.0.1","port":13333,"showInHome":true,"routeTargets":[]}"#).unwrap();
    let v=capture(&db,"known good").unwrap();
    let mut provider=db.get_provider("gov-test").unwrap().unwrap(); provider.name="changed".into(); db.save_provider(&provider).unwrap();
    let mut newer=provider.clone(); newer.id="newer".into(); db.save_provider(&newer).unwrap();
    db.set_setting("security-outside-snapshot","do not change").unwrap();
    let issued=crate::governance::access::issue(&db,"test monitor","monitor",30).unwrap();
    let p=preview(&db,&v.id).unwrap(); assert!(!p.after.contains(&issued.token)); assert!(!p.after.contains("budget_ledger"));
    let backup=restore(&db,&v.id,&p.expected_current_hash,&p.target_hash).unwrap(); assert_eq!(count(&db),2);
    assert_eq!(db.get_provider("gov-test").unwrap().unwrap().name,"Governance test"); assert!(!db.get_provider("newer").unwrap().unwrap().enabled);
    assert_eq!(db.get_setting("security-outside-snapshot").unwrap().as_deref(),Some("do not change"));
    let proxy:crate::proxy_manager::ProxySettings=serde_json::from_str(&db.get_setting("proxy_settings").unwrap().unwrap()).unwrap(); assert!(!proxy.enabled);
    let back=preview(&db,&backup.id).unwrap(); assert!(back.after.contains("changed"));
}
#[test]
fn transaction_failure_changes_nothing_including_automatic_backup() {
    let db=setup(); db.save_prompt(&Prompt{id:"p".into(),name:"Original".into(),content:"old".into(),description:None,enabled:true}).unwrap();
    let v=capture(&db,"with prompt").unwrap(); db.save_prompt(&Prompt{id:"p".into(),name:"Edited".into(),content:"new".into(),description:None,enabled:true}).unwrap();
    let p=preview(&db,&v.id).unwrap();
    { let c=db.conn(); let c=c.lock().unwrap(); c.execute_batch("CREATE TRIGGER fail_restore BEFORE INSERT ON prompts BEGIN SELECT RAISE(ABORT,'injected failure'); END;").unwrap(); }
    assert!(restore(&db,&v.id,&p.expected_current_hash,&p.target_hash).is_err());
    assert_eq!(count(&db),1); assert_eq!(db.list_prompts().unwrap()[0].content,"new");
}
#[test]
fn version_storage_and_labels_are_bounded() {
    let db=setup(); assert!(capture(&db,"").is_err()); assert!(capture(&db,"line\nbreak").is_err());
    { let c=db.conn(); let c=c.lock().unwrap(); for n in 0..200 { c.execute("INSERT INTO config_versions VALUES(?1,'v','{}','hash',0)",[n.to_string()]).unwrap(); } }
    assert!(capture(&db,"full").unwrap_err().contains("delete an old version"));
}
