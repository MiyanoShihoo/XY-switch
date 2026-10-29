use super::*;
struct Root(PathBuf);
impl Root {
    fn new() -> Self { let path=std::env::temp_dir().join(format!("xy-mount-{}",uuid::Uuid::new_v4())); fs::create_dir(&path).unwrap(); Self(path) }
}
impl Drop for Root {
    fn drop(&mut self) {
        for name in ["AGENTS.md","CLAUDE.md","GEMINI.md"] { let p=self.0.join(name); if p.is_dir() { let _=fs::remove_dir(p); } else { let _=fs::remove_file(p); } }
        let _=fs::remove_dir(&self.0);
    }
}
#[test]
fn managed_block_preserves_manual_content_and_rejects_ambiguous_markers() {
    assert_eq!(merge("manual\r\n","hello").unwrap(),format!("manual\r\n{BEGIN}\nhello\n{END}\n"));
    assert_eq!(merge(&format!("before\n{BEGIN}\nold\n{END}\nafter"),"new").unwrap(),format!("before\n{BEGIN}\nnew\n{END}\nafter"));
    for malformed in [format!("{END}{BEGIN}"),format!("{BEGIN}twice{BEGIN}{END}"),BEGIN.into(),END.into()] { assert!(merge(&malformed,"hello").is_err()); }
    assert!(merge("",BEGIN).is_err()); assert!(merge("",&"x".repeat(MAX_BYTES)).is_err());
}
#[test]
fn preview_does_not_write_then_backup_restore_and_one_use_are_enforced() {
    let dir=Root::new(); let db=crate::governance::test_support::database(); let manager=MountManager::default();
    let root=manager.grant(dir.0.clone()).unwrap(); let target=dir.0.join("AGENTS.md");
    fs::write(&target,"manual\r\n").unwrap();
    let p=manager.preview(&root.id,"codex","mounted").unwrap();
    assert_eq!(fs::read_to_string(&target).unwrap(),"manual\r\n");
    assert!(manager.apply(&db,&p.id,"wrong").is_err());
    let backup=manager.apply(&db,&p.id,"WRITE FILE").unwrap();
    assert_eq!(fs::read_to_string(&target).unwrap(),p.after);
    assert!(manager.apply(&db,&p.id,"WRITE FILE").is_err());
    let rollback=manager.rollback_preview(&db,&root.id,&backup.id).unwrap();
    assert_eq!(fs::read_to_string(&target).unwrap(),p.after,"rollback preview must also be read-only");
    manager.apply(&db,&rollback.id,"WRITE FILE").unwrap();
    assert_eq!(fs::read_to_string(&target).unwrap(),"manual\r\n");
}
#[test]
fn rollback_of_new_file_deletes_only_after_explicit_confirmation() {
    let dir=Root::new(); let db=crate::governance::test_support::database(); let manager=MountManager::default();
    let root=manager.grant(dir.0.clone()).unwrap(); let target=dir.0.join("CLAUDE.md");
    let p=manager.preview(&root.id,"claude","text").unwrap(); assert!(!target.exists()); assert!(p.creates_file);
    let b=manager.apply(&db,&p.id,"WRITE FILE").unwrap();
    let r=manager.rollback_preview(&db,&root.id,&b.id).unwrap(); assert!(r.removes_file); assert!(target.exists());
    assert!(manager.apply(&db,&r.id,"").is_err()); assert!(target.exists());
    manager.apply(&db,&r.id,"WRITE FILE").unwrap(); assert!(!target.exists());
}
#[test]
fn external_edits_expiry_and_wrong_directory_never_get_overwritten() {
    let dir=Root::new(); let other=Root::new(); let db=crate::governance::test_support::database(); let manager=MountManager::default();
    let root=manager.grant(dir.0.clone()).unwrap(); let target=dir.0.join("GEMINI.md");
    let p=manager.preview(&root.id,"gemini","text").unwrap(); fs::write(&target,"external").unwrap();
    assert!(manager.apply(&db,&p.id,"WRITE FILE").is_err()); assert_eq!(fs::read_to_string(&target).unwrap(),"external");
    let p=manager.preview(&root.id,"gemini","text").unwrap(); manager.pending.lock().unwrap().get_mut(&p.id).unwrap().expires=now()-1;
    assert!(manager.apply(&db,&p.id,"WRITE FILE").is_err()); assert_eq!(fs::read_to_string(&target).unwrap(),"external");
    let p=manager.preview(&root.id,"gemini","text").unwrap(); let b=manager.apply(&db,&p.id,"WRITE FILE").unwrap();
    let other=manager.grant(other.0.clone()).unwrap(); assert!(manager.rollback_preview(&db,&other.id,&b.id).is_err());
    fs::write(&target,"a later external edit").unwrap(); assert!(manager.rollback_preview(&db,&root.id,&b.id).is_err());
    assert_eq!(fs::read_to_string(&target).unwrap(),"a later external edit");
}
#[test]
fn only_whitelisted_utf8_regular_files_are_eligible() {
    let dir=Root::new(); let manager=MountManager::default(); let root=manager.grant(dir.0.clone()).unwrap();
    assert!(manager.preview(&root.id,"../AGENTS.md","text").is_err());
    fs::create_dir(dir.0.join("AGENTS.md")).unwrap(); assert!(manager.preview(&root.id,"codex","text").is_err());
    fs::write(dir.0.join("CLAUDE.md"),[0xff,0xfe,0xfa]).unwrap(); assert!(manager.preview(&root.id,"claude","text").is_err());
    assert!(manager.preview("ungranted","gemini","text").is_err());
}
