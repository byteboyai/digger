#[cfg(test)]
mod tests {
 use bytehost_git_accounts::*;
 use bytehost_settings::git::GitPageState;
 use std::sync::{Arc,Barrier,Mutex,atomic::{AtomicUsize,Ordering}};
 #[derive(Clone,Default)] struct Cred(Arc<MemoryCredentialStore>);
 impl CredentialStore for Cred {
  fn get(&self,p:GitProvider)->Result<Option<SecretToken>,GitAccountError>{self.0.get(p)}
  fn set(&self,p:GitProvider,t:&SecretToken)->Result<(),GitAccountError>{self.0.set(p,t)}
  fn delete(&self,p:GitProvider)->Result<(),GitAccountError>{self.0.delete(p)}
 }
 struct HeldHttp {entered:std::sync::mpsc::Sender<()>, release:Mutex<Option<tokio::sync::oneshot::Receiver<()>>>}
 impl HttpTransport for HeldHttp {
  async fn send(&self,_:HttpRequest)->Result<HttpResponse,GitAccountError>{
   self.entered.send(()).unwrap();
   let rx=self.release.lock().unwrap().take().unwrap(); rx.await.unwrap();
   Ok(HttpResponse{status:200,body:r#"{"login":"synthetic-user"}"#.into()})
  }
 }
 #[test] fn cancelling_the_page_does_not_cancel_credential_commit(){
  let credentials=Cred::default(); let (entered,waiting)=std::sync::mpsc::channel(); let (release,rx)=tokio::sync::oneshot::channel();
  let creds=credentials.clone(); let mut page=GitPageState::new(1);
  page.begin_edit(GitProvider::GitHub);page.set_token(GitProvider::GitHub,"SYNTHETIC-NOT-A-REAL-TOKEN");
  let ticket=page.begin_connect(GitProvider::GitHub).unwrap();
  let thread=std::thread::spawn(move||{
   let service=AccountService::new(creds,MemoryMetadataStore::default(),HeldHttp{entered,release:Mutex::new(Some(rx))});
   tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(service.connect_with_account(GitProvider::GitHub,&SecretToken::new("SYNTHETIC-NOT-A-REAL-TOKEN")))
  });
  waiting.recv().unwrap();page.cancel(GitProvider::GitHub); release.send(()).unwrap();
  let (account,outcome)=thread.join().unwrap().unwrap();
  assert!(credentials.get(GitProvider::GitHub).unwrap().is_some());
  assert!(!page.connect_succeeded(ticket,account,outcome));assert!(page.account(GitProvider::GitHub).is_none());
  println!("REPRO: cancelled request persisted credential; UI rejected committed result");
 }
 #[derive(Clone)] struct RacingMetadata {disk:Arc<JsonFileMetadataStore>,ready:Arc<Barrier>,loads:Arc<AtomicUsize>,save_lock:Arc<Mutex<()>>}
 impl MetadataStore for RacingMetadata {
  fn load(&self)->Result<GitAccountsState,GitAccountError>{let state=self.disk.load()?;if self.loads.fetch_add(1,Ordering::SeqCst)<2{self.ready.wait();}Ok(state)}
  fn save(&self,s:&GitAccountsState)->Result<(),GitAccountError>{let _g=self.save_lock.lock().unwrap();self.disk.save(s)}
 }
 #[test] fn two_provider_connections_lose_one_metadata_record(){
  let path=std::env::temp_dir().join(format!("digger-unified-race-{}.json",std::process::id()));let _=std::fs::remove_file(&path);
  let metadata=RacingMetadata{disk:Arc::new(JsonFileMetadataStore::new(&path)),ready:Arc::new(Barrier::new(2)),loads:Arc::new(AtomicUsize::new(0)),save_lock:Arc::new(Mutex::new(()))};
  let creds=Cred::default();
  let threads:Vec<_>=[GitProvider::GitHub,GitProvider::Gitee].into_iter().map(|p|{
   let (m,c)=(metadata.clone(),creds.clone());std::thread::spawn(move||{
    let http=FakeTransport::new().push_json("https://",200,r#"{"login":"synthetic-user"}"#);let service=AccountService::new(c,m,http);
    tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(service.connect(p,&SecretToken::new("SYNTHETIC-NOT-A-REAL-TOKEN"))).unwrap()
   })
  }).collect();for t in threads{assert!(t.join().unwrap().metadata_committed);}
  let state=metadata.disk.load().unwrap();assert_eq!(GitProvider::ALL.into_iter().filter(|p|state.get(*p).is_some()).count(),1);
  assert!(creds.get(GitProvider::GitHub).unwrap().is_some());assert!(creds.get(GitProvider::Gitee).unwrap().is_some());
  println!("REPRO: two successful provider commits leave only one metadata entry");std::fs::remove_file(path).unwrap();
 }
}
