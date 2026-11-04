#[cfg(test)]
mod tests {
 use bytehost_git_accounts::*;
 use std::sync::{Arc,Mutex};
 #[derive(Clone)] struct HeldCredentials {
  entries:Arc<MemoryCredentialStore>,
  entered:std::sync::mpsc::Sender<()>,
  resume:Arc<Mutex<std::sync::mpsc::Receiver<()>>>,
 }
 impl CredentialStore for HeldCredentials {
  fn get(&self,p:GitProvider)->Result<Option<SecretToken>,GitAccountError>{self.entries.get(p)}
  fn set(&self,p:GitProvider,t:&SecretToken)->Result<(),GitAccountError>{
   if t.expose()=="SYNTHETIC-OLD" {self.entered.send(()).unwrap();self.resume.lock().unwrap().recv().unwrap();}
   self.entries.set(p,t)
  }
  fn delete(&self,p:GitProvider)->Result<(),GitAccountError>{self.entries.delete(p)}
 }
 #[derive(Clone,Default)] struct SharedMeta(Arc<MemoryMetadataStore>);
 impl MetadataStore for SharedMeta {
  fn load(&self)->Result<GitAccountsState,GitAccountError>{self.0.load()}
  fn save(&self,s:&GitAccountsState)->Result<(),GitAccountError>{self.0.save(s)}
  fn update(&self,f:&mut dyn FnMut(&mut GitAccountsState))->Result<GitAccountsState,GitAccountError>{self.0.update(f)}
 }
 #[test] fn old_admitted_commit_overwrites_new_provider_request(){
  let (entered,waiting)=std::sync::mpsc::channel();let (release,resume)=std::sync::mpsc::channel();
  let creds=HeldCredentials{entries:Arc::new(MemoryCredentialStore::default()),entered,resume:Arc::new(Mutex::new(resume))};
  let meta=SharedMeta::default();let admission=Arc::new(Admission::new());
  let old=RequestTicket{owner:1,provider:GitProvider::GitHub,seq:1};admission.register(old);
  let (c,m,a)=(creds.clone(),meta.clone(),admission.clone());
  let thread=std::thread::spawn(move||{
   let http=FakeTransport::new().push_json("https://",200,r#"{"login":"old-user"}"#);
   let service=AccountService::new_with_admission(c,m,http,a);
   tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(service.connect_with_account_at(GitProvider::GitHub,&SecretToken::new("SYNTHETIC-OLD"),Some(old))).unwrap()
  });
  waiting.recv().unwrap();
  let new=RequestTicket{seq:2,..old};admission.register(new);
  let http=FakeTransport::new().push_json("https://",200,r#"{"login":"new-user"}"#);
  let service=AccountService::new_with_admission(creds.clone(),meta.clone(),http,admission.clone());
  let (_,outcome)=tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(service.connect_with_account_at(GitProvider::GitHub,&SecretToken::new("SYNTHETIC-NEW"),Some(new))).unwrap();
  assert!(outcome.is_complete());assert!(!admission.admit(old));
  release.send(()).unwrap();assert!(thread.join().unwrap().1.is_complete());
  assert_eq!(creds.get(GitProvider::GitHub).unwrap().unwrap().expose(),"SYNTHETIC-OLD");
  assert_eq!(meta.load().unwrap().get(GitProvider::GitHub).unwrap().username,"old-user");
  println!("REPRO: old admitted commit overwrote a newer successful request");
 }
 struct FailSave;
 impl MetadataStore for FailSave {
  fn load(&self)->Result<GitAccountsState,GitAccountError>{Ok(GitAccountsState::default())}
  fn save(&self,_:&GitAccountsState)->Result<(),GitAccountError>{Err(GitAccountError::MetadataWrite{detail:"synthetic failure".into()})}
 }
 #[test] fn committed_credential_disappears_from_authoritative_query_after_metadata_failure(){
  let (entered,_waiting)=std::sync::mpsc::channel();let (_release,resume)=std::sync::mpsc::channel();
  let creds=HeldCredentials{entries:Arc::new(MemoryCredentialStore::default()),entered,resume:Arc::new(Mutex::new(resume))};
  let service=AccountService::new(creds.clone(),FailSave,FakeTransport::new().push_json("https://",200,r#"{"login":"user"}"#));
  let (_,outcome)=tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(service.connect_with_account(GitProvider::GitHub,&SecretToken::new("SYNTHETIC-NEW"))).unwrap();
  assert!(outcome.credential_committed);assert!(!outcome.metadata_committed);
  assert!(creds.get(GitProvider::GitHub).unwrap().is_some());
  assert!(service.query().unwrap().accounts.get(GitProvider::GitHub).is_none());
  println!("REPRO: authoritative query hides the already committed credential");
 }
 #[derive(Clone)] struct PairCredentials {inner:Arc<MemoryCredentialStore>,barrier:Arc<std::sync::Barrier>}
 impl CredentialStore for PairCredentials {
  fn get(&self,p:GitProvider)->Result<Option<SecretToken>,GitAccountError>{self.inner.get(p)}
  fn set(&self,p:GitProvider,t:&SecretToken)->Result<(),GitAccountError>{self.inner.set(p,t)?;self.barrier.wait();Ok(())}
  fn delete(&self,p:GitProvider)->Result<(),GitAccountError>{self.inner.delete(p)}
 }
 #[test] fn fixed_json_updates_keep_both_provider_records_with_real_thread_overlap(){
  let path=std::env::temp_dir().join(format!("digger-round2-provider-json-{}.json",std::process::id()));
  let _=std::fs::remove_file(&path);
  let creds=PairCredentials{inner:Arc::new(MemoryCredentialStore::default()),barrier:Arc::new(std::sync::Barrier::new(2))};
  let threads:Vec<_>=[GitProvider::GitHub,GitProvider::Gitee].into_iter().map(|p|{
   let c=creds.clone();let path=path.clone();std::thread::spawn(move||{
    let service=AccountService::new(c,JsonFileMetadataStore::new(path),FakeTransport::new().push_json("https://",200,r#"{"login":"user"}"#));
    tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(service.connect(p,&SecretToken::new("SYNTHETIC"))).unwrap()
   })
  }).collect();
  for thread in threads{assert!(thread.join().unwrap().is_complete());}
  let state=JsonFileMetadataStore::new(&path).load().unwrap();
  assert!(state.get(GitProvider::GitHub).is_some());assert!(state.get(GitProvider::Gitee).is_some());
  std::fs::remove_file(path).unwrap();println!("FIX VERIFIED: overlapping provider updates retain both JSON records");
 }

}
