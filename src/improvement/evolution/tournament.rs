//! Independently score sibling proposals from the same adopted base before
//! electing a successor. Model prompts never receive the hold-out verifier
//! command or its observations. Only Rust sees that additional evidence.
use super::*;
use crate::improvement::{git,relative,Measurement,NewImprovement,ImprovementStatus};
use std::path::PathBuf;

fn choose<'a>(items:&'a [CandidateRecord])->Option<&'a CandidateRecord>{
    items.iter().filter(|c|c.eligible && c.training_score.is_some()).max_by(|a,b|{
        let aval=a.validation_score.or(a.training_score).unwrap_or(f64::NEG_INFINITY);
        let bval=b.validation_score.or(b.training_score).unwrap_or(f64::NEG_INFINITY);
        aval.total_cmp(&bval)
            .then_with(||a.training_score.unwrap_or(f64::NEG_INFINITY)
                .total_cmp(&b.training_score.unwrap_or(f64::NEG_INFINITY)))
            .then_with(||b.proposal_index.cmp(&a.proposal_index))
    })
}
impl Improvements {
    pub(super) async fn tournament_generation(
        &self, model:&ModelClient,adopted:&Improvement,target:&str,
        run:&mut EvolutionRun
    )->Result<Option<Uuid>,String>{
        let generation=run.completed_generations;
        run.candidate_groups.push(Vec::new());
        self.put_evolution(run.clone()).await?;
        let mut evaluated=Vec::<CandidateRecord>::new();
        let mut shared_baseline:Option<(String,Measurement)>=None;
        let mut shared_validation:Option<(String,Measurement)>=None;
        for proposal_index in 0..run.candidates_per_generation {
            if self.should_stop_evolution(run).await{return Ok(None);}
            if run.spent_krw>=run.cost_budget_krw {
                run.status=EvolutionStatus::NeedsReview;
                run.stop_reason=Some("Candidate tournament reached measured KRW ceiling".into());
                return Ok(None);
            }
            // Every sibling has the same adopted parent and original base HEAD.
            // No candidate is adopted until the entire tournament has been
            // independently checked and a winner selected.
            let mut trial=self.create(NewImprovement{
                repository:adopted.repository.clone(),target_file:target.into(),
                instruction:adopted.instruction.clone(),test_command:adopted.test_command.clone(),
                score_command:adopted.score_command.clone(),parent:Some(adopted.id)
            }).await?;
            trial.proposal_index=proposal_index;
            trial.proposal_count=run.candidates_per_generation;
            trial.status=ImprovementStatus::Running;
            trial.updated_at_ms=now_ms();
            self.put(trial.clone()).await?;
            run.candidate_groups[generation].push(trial.id);
            run.updated_at_ms=now_ms();
            self.put_evolution(run.clone()).await?;
            let baseline=shared_baseline.as_ref().map(|(sha,result)|(sha.as_str(),result));
            let result=self.evaluate_with_baseline(model,trial.clone(),baseline).await;
            if let Err(error)=&result {
                if let Some(mut record)=self.get(trial.id).await {
                    record.status=ImprovementStatus::Failed;
                    record.error=Some(error.clone());
                    record.updated_at_ms=now_ms();
                    self.put(record).await?;
                }
            }
            let result_trial=self.get(trial.id).await.ok_or("Candidate record lost")?;
            if shared_baseline.is_none(){
                if let (Some(sha),Some(baseline))=(
                    result_trial.base_commit.as_ref(),result_trial.baseline.as_ref()
                ){
                    shared_baseline=Some((sha.clone(),baseline.clone()));
                }
            }
            run.spent_krw+=result_trial.cost_krw;
            let mut candidate=CandidateRecord{
                trial_id:trial.id,generation,proposal_index,
                training_score:result_trial.candidate.as_ref().and_then(|x|x.score),
                validation_baseline_score:None,validation_score:None,
                eligible:result_trial.status==ImprovementStatus::Evaluated && result_trial.accepted,
                reason:result.err(),
            };
            // Keep the numeric holdout command out of the model-facing
            // candidate prompts and reflection. It is operator-frozen per run.
            if candidate.eligible {
                if let Some(command)=run.validation_command.as_deref() {
                    let cached=shared_validation.as_ref()
                        .map(|(sha,measurement)|(sha.as_str(),measurement));
                    match self.holdout(&result_trial,command,cached).await {
                        Ok((baseline,proposed))=>{
                            if shared_validation.is_none(){
                                shared_validation=Some((
                                    result_trial.base_commit.clone().ok_or("Missing holdout base")?,
                                    baseline.clone()
                                ));
                            }
                            candidate.validation_baseline_score=baseline.score;
                            candidate.validation_score=proposed.score;
                            if !super::super::is_better(&baseline,&proposed) {
                                candidate.eligible=false;
                                candidate.reason=Some("Independent validation did not improve".into());
                            }
                        }
                        Err(error)=>{
                            candidate.eligible=false;
                            candidate.reason=Some(format!("Validation error: {error}"));
                        }
                    }
                }
            }
            if !candidate.eligible && candidate.reason.is_none() {
                candidate.reason=Some("Training score did not improve".into());
            }
            // A candidate rejected by independent validation is no longer
            // independently adoptable through the ordinary trial endpoint.
            if !candidate.eligible && result_trial.status==ImprovementStatus::Evaluated {
                let mut rejected=result_trial.clone();
                rejected.accepted=false;
                rejected.status=ImprovementStatus::Rejected;
                rejected.error=candidate.reason.clone();
                rejected.updated_at_ms=now_ms();
                self.put(rejected).await?;
            }
            evaluated.push(candidate.clone());
            run.candidates.push(candidate);
            run.updated_at_ms=now_ms();
            self.put_evolution(run.clone()).await?;
            // No candidate may be adopted after a stop, including a pending
            // evaluation request or a single over-budget provider response.
            if self.should_stop_evolution(run).await{return Ok(None);}
            if run.spent_krw>run.cost_budget_krw {
                run.status=EvolutionStatus::NeedsReview;
                run.stop_reason=Some("Tournament exceeded measured KRW budget; no adoption".into());
                return Ok(None);
            }
        }
        let winner=choose(&evaluated).map(|item|item.trial_id);
        // Candidates that passed a threshold but lost the competition must
        // not stay marked as independently adoptable through the public API.
        for item in evaluated.iter().filter(|item|item.eligible && Some(item.trial_id)!=winner) {
            if let Some(mut lost)=self.get(item.trial_id).await {
                lost.accepted=false;
                lost.status=ImprovementStatus::Rejected;
                lost.reflection=Some(format!(
                    "{} Tournament selection: higher-scored sibling was preferred.",
                    lost.reflection.unwrap_or_default()
                ));
                lost.updated_at_ms=now_ms();
                self.put(lost).await?;
            }
        }
        Ok(winner)
    }

    async fn holdout(
        &self,trial:&Improvement,command:&str,
        frozen_baseline:Option<(&str,&Measurement)>
    )->Result<(Measurement,Measurement),String>{
        let workspace=self.workspace.canonicalize().map_err(|e|e.to_string())?;
        let repo=workspace.join(relative(&trial.repository)?).canonicalize()
            .map_err(|e|e.to_string())?;
        if !repo.starts_with(&workspace){return Err("Holdout repository escaped workspace".into());}
        let base=trial.base_commit.as_deref().ok_or("Missing candidate base")?;
        if frozen_baseline.is_some_and(|(sha,_)|sha!=base){
            return Err("Cached validation baseline has a different Git revision".into());
        }
        let proposed=trial.candidate_commit.as_deref().ok_or("Missing pinned candidate")?;
        let pinned=git(&repo,&["rev-parse",&format!("refs/dominion/improvements/{}",trial.id)]).await?;
        if pinned.trim()!=proposed {return Err("Holdout candidate differs from pinned Git commit".into());}
        let root=self.root.join(trial.id.to_string());
        tokio::fs::create_dir_all(&root).await.map_err(|e|e.to_string())?;
        let baseline=root.join("holdout_baseline");
        let candidate=root.join("holdout_candidate");
        let mut created=Vec::<PathBuf>::new();
        let worktrees=if frozen_baseline.is_some(){vec![(&candidate,proposed)]}
            else{vec![(&baseline,base),(&candidate,proposed)]};
        for (path,sha) in worktrees {
            let name=path.to_str().ok_or("Invalid validation worktree path")?;
            if let Err(error)=git(&repo,&["worktree","add","--detach",name,sha]).await {
                for existing in &created {
                    if let Some(dir)=existing.to_str(){let _=git(&repo,&["worktree","remove","--force",dir]).await;}
                }
                return Err(error);
            }
            created.push(path.to_path_buf());
        }
        let result=async {
            let a=match frozen_baseline {
                Some((_,previous))=>previous.clone(),
                None=>self.measure(&baseline,&trial.test_command,Some(command)).await?
            };
            let b=self.measure(&candidate,&trial.test_command,Some(command)).await?;
            Ok::<_,String>((a,b))
        }.await;
        for dir in [&baseline,&candidate] {
            if let Some(name)=dir.to_str(){
                let _=git(&repo,&["worktree","remove","--force",name]).await;
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn real_git_competition_rejects_high_training_score_that_fails_independent_validation(){
        use crate::improvement::{execute,NewImprovement};
        use axum::{routing::post,Json,Router};
        use serde_json::{json,Value};
        use tokio::process::Command;

        async fn alternative(Json(request):Json<Value>)->Json<Value>{
            let text=request["messages"].as_array()
                .and_then(|messages|messages.iter().rev().find(|msg|msg["role"]=="user"))
                .and_then(|item|item["content"].as_str()).unwrap_or("");
            let current=text.split("Current contents:\n").last()
                .unwrap_or("0").trim().parse::<u32>().unwrap_or(0);
            let jump=if text.contains("Search alternative 2 of 2") {25} else {1};
            Json(json!({"choices":[{"message":{"role":"assistant",
                "content":format!("{}\n",current+jump)}}],
                "usage":{"prompt_tokens":7,"completion_tokens":3}}))
        }

        if Command::new("git").arg("--version").output().await.is_err(){return;}
        let root=std::env::temp_dir().join(format!("dominion-competition-{}",Uuid::new_v4()));
        let workspace=root.join("workspace");
        let repo=workspace.join("project");
        tokio::fs::create_dir_all(&repo).await.unwrap();
        git(&repo,&["init"]).await.unwrap();
        tokio::fs::write(repo.join("a.txt"),"0\n").await.unwrap();
        tokio::fs::write(repo.join("b.txt"),"0\n").await.unwrap();
        git(&repo,&["add","a.txt","b.txt"]).await.unwrap();
        execute("git",&["-c","user.name=Test","-c","user.email=test@example.com",
            "commit","-m","original"],&repo,30).await.unwrap();

        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr=listener.local_addr().unwrap();
        let server=tokio::spawn(async move {
            axum::serve(listener,Router::new().route("/chat/completions",post(alternative)))
                .await.unwrap();
        });
        let model=crate::provider::ModelClient::new(
            crate::provider::ModelConfig::basic(
                format!("http://{addr}/chat/completions"),"contest".into(),None
            ),None
        ).unwrap();
        let records=root.join("records");
        let service=Arc::new(Improvements::open(records.clone(),workspace.clone()).await.unwrap());
        let seed=service.create(NewImprovement{
            repository:"project".into(),target_file:"a.txt".into(),
            instruction:"Find a measurable implementation improvement".into(),
            test_command:"test -f a.txt && test -f b.txt".into(),
            score_command:Some("echo $(( $(cat a.txt) + $(cat b.txt) ))".into()),
            parent:None
        }).await.unwrap();
        service.start(model.clone(),seed.id,true).await.unwrap();
        for _ in 0..150{
            if service.get(seed.id).await.unwrap().status!=ImprovementStatus::Running {break;}
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let seed=service.adopt(seed.id).await.unwrap();
        assert_eq!(tokio::fs::read_to_string(repo.join("a.txt")).await.unwrap(),"1\n");
        let run=service.evolution_start(model,seed.id,NewEvolution{
            max_generations:1,cost_budget_krw:100.0,
            target_files:vec!["b.txt".into()],
            candidates_per_generation:2,
            // The second candidate gets a much higher TRAINING score, but
            // its 25 causes this operator-provided validation to fail.
            validation_command:Some("test $(cat b.txt) -lt 10 && echo $(( $(cat a.txt) + $(cat b.txt) ))".into())
        },true).await.unwrap();
        let mut terminal=None;
        for _ in 0..180{
            let current=service.evolution(run.id).await.unwrap();
            if current.status!=EvolutionStatus::Running{terminal=Some(current);break;}
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let done=terminal.expect("Candidate tournament did not settle");
        assert_eq!(done.status,EvolutionStatus::Completed,"{:?}",done.stop_reason);
        assert_eq!(done.completed_generations,1);
        assert_eq!(done.candidates.len(),2);
        assert_eq!(done.candidate_groups.len(),1);
        assert_eq!(done.candidate_groups[0].len(),2);
        let winner=done.descendants[0];
        assert_eq!(winner,done.candidate_groups[0][0]);
        let rejected=service.get(done.candidate_groups[0][1]).await.unwrap();
        assert_eq!(rejected.status,ImprovementStatus::Rejected);
        assert!(!rejected.accepted);
        assert_eq!(done.candidates[0].training_score,Some(2.0));
        assert_eq!(done.candidates[0].validation_score,Some(2.0));
        assert_eq!(done.candidates[1].training_score,Some(26.0));
        assert!(!done.candidates[1].eligible);
        assert_eq!(tokio::fs::read_to_string(repo.join("b.txt")).await.unwrap(),"1\n");
        let persisted=Improvements::open(records,workspace).await.unwrap();
        assert_eq!(persisted.evolution(run.id).await.unwrap().candidate_groups[0].len(),2);
        server.abort();
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[test]
    fn independent_validation_beats_training_and_ties_are_stable(){
        let a=CandidateRecord{trial_id:Uuid::new_v4(),generation:0,proposal_index:0,
            training_score:Some(9.0),validation_baseline_score:Some(0.0),
            validation_score:Some(1.0),eligible:true,reason:None};
        let b=CandidateRecord{trial_id:Uuid::new_v4(),generation:0,proposal_index:1,
            training_score:Some(5.0),validation_baseline_score:Some(0.0),
            validation_score:Some(3.0),eligible:true,reason:None};
        assert_eq!(choose(&[a.clone(),b.clone()]).unwrap().trial_id,b.trial_id);
        let mut invalid=b.clone();
        invalid.eligible=false;
        assert_eq!(choose(&[a.clone(),invalid]).unwrap().trial_id,a.trial_id);
        let tied=CandidateRecord{validation_score:Some(1.0),training_score:Some(9.0),..b};
        assert_eq!(choose(&[a.clone(),tied]).unwrap().trial_id,a.trial_id);
    }
}
