import Yasumaro.ModelLifecycle

namespace Yasumaro

theorem verified_initial_safe (partialPresent : Bool) (readers : Nat) :
    Safe {
      published := true
      publishedVerified := true
      «partial» := partialPresent
      partialVerified := false
      publishAuthorized := false
      cancelRequested := false
      readers := readers
      writer := false
    } := by
  simp [Safe]

theorem step_preserves (state : State) (event : Event)
    (safe : Safe state) : Safe (step state event) := by
  cases event with
  | beginInstall =>
      by_cases available : state.readers = 0 ∧ state.writer = false ∧
          state.cancelRequested = false
      · rw [step, ite_eq_left available]
        unfold Safe
        exact ⟨safe.1, safe.2.1, by simp, fun _ => available.1⟩
      · rw [step, ite_eq_right available]
        exact safe
  | createPartial =>
      by_cases writer : state.writer = true
      · rw [step, ite_eq_left writer]
        unfold Safe
        exact ⟨safe.1, by simp, by simp, safe.2.2.2⟩
      · rw [step, ite_eq_right writer]
        exact safe
  | verifyPartial =>
      by_cases ready : state.writer = true ∧ state.«partial» = true
      · rw [step, ite_eq_left ready]
        unfold Safe
        refine ⟨safe.1, (fun _ => ready.2), ?_, safe.2.2.2⟩
        intro authorized
        exact ⟨rfl, (safe.2.2.1 authorized).2⟩
      · rw [step, ite_eq_right ready]
        exact safe
  | requestCancel =>
      simpa [step, Safe] using safe
  | authorizePublish =>
      by_cases ready : state.writer = true ∧ state.«partial» = true ∧
          state.partialVerified = true ∧ state.cancelRequested = false
      · rw [step, ite_eq_left ready]
        unfold Safe
        exact ⟨safe.1, safe.2.1, fun _ => ⟨ready.2.2.1, ready.1⟩,
          safe.2.2.2⟩
      · rw [step, ite_eq_right ready]
        exact safe
  | publish =>
      by_cases ready : state.writer = true ∧ state.«partial» = true ∧
          state.partialVerified = true ∧ state.publishAuthorized = true
      · simp [step, ready, Safe]
      · rw [step, ite_eq_right ready]
        exact safe
  | abort cleanupSucceeded =>
      by_cases writer : state.writer = true
      · by_cases cleanup : cleanupSucceeded = true
        · rw [step, ite_eq_left writer]
          simp [cleanup, Safe]
          exact safe.1
        · rw [step, ite_eq_left writer]
          simp only [cleanup, Bool.false_eq_true, ite_false]
          unfold Safe
          exact ⟨safe.1, safe.2.1, by simp, by simp⟩
      · rw [step, ite_eq_right writer]
        exact safe
  | acquireLease =>
      by_cases available : state.writer = false ∧ state.published = true ∧
          state.publishedVerified = true
      · rw [step, ite_eq_left available]
        unfold Safe
        refine ⟨safe.1, safe.2.1, ?_, ?_⟩
        intro authorized
        have writer := (safe.2.2.1 authorized).2
        simp_all
        intro writer
        simp_all
      · rw [step, ite_eq_right available]
        exact safe
  | releaseLease =>
      by_cases hasReader : 0 < state.readers
      · rw [step, ite_eq_left hasReader]
        unfold Safe
        refine ⟨safe.1, safe.2.1, safe.2.2.1, ?_⟩
        intro writer
        have noReaders := safe.2.2.2 writer
        omega
      · rw [step, ite_eq_right hasReader]
        exact safe
  | remove =>
      by_cases available : state.writer = false ∧ state.readers = 0
      · simp [step, available, Safe]
      · rw [step, ite_eq_right available]
        exact safe

theorem run_preserves (state : State) (events : List Event)
    (safe : Safe state) : Safe (run state events) := by
  induction events generalizing state with
  | nil => simpa [run]
  | cons event events ih =>
      simp only [run]
      exact ih (step state event) (step_preserves state event safe)

theorem cleanup_success_clears_partial (state : State)
    (writer : state.writer = true) :
    (step state (.abort true)).«partial» = false := by
  simp [step, writer]

theorem cleanup_failure_preserves_publication (state : State) :
    (step state (.abort false)).published = state.published ∧
      (step state (.abort false)).publishedVerified = state.publishedVerified := by
  by_cases writer : state.writer = true <;> simp [step, writer]

theorem unverified_partial_cannot_be_authorized (state : State)
    (unauthorized : state.publishAuthorized = false)
    (unverified : state.partialVerified = false) :
    (step state .authorizePublish).publishAuthorized = false := by
  simp [step, unverified, unauthorized]

theorem cancel_before_authorization_blocks_publication (state : State)
    (unauthorized : state.publishAuthorized = false)
    (cancelled : state.cancelRequested = true) :
    step (step state .authorizePublish) .publish =
      step state .authorizePublish := by
  simp [step, cancelled, unauthorized]

theorem cancel_after_authorization_preserves_authorization (state : State)
    (authorized : state.publishAuthorized = true) :
    (step state .requestCancel).publishAuthorized = true := by
  simpa [step] using authorized

private def brokenUnverifiedAuthorizationWitness : State :=
  { published := false
    publishedVerified := false
    «partial» := true
    partialVerified := false
    publishAuthorized := false
    cancelRequested := false
    readers := 0
    writer := true }

theorem broken_unverified_authorization_violates_safety :
    Safe (step brokenUnverifiedAuthorizationWitness .authorizePublish) ∧
      ¬Safe (brokenUnverifiedAuthorizeStep brokenUnverifiedAuthorizationWitness
        .authorizePublish) := by
  simp [brokenUnverifiedAuthorizationWitness, step,
    brokenUnverifiedAuthorizeStep, Safe]

private def brokenCancelAuthorizationWitness : State :=
  { published := false
    publishedVerified := false
    «partial» := true
    partialVerified := true
    publishAuthorized := false
    cancelRequested := true
    readers := 0
    writer := true }

theorem broken_cancel_authorization_is_detected :
    (step brokenCancelAuthorizationWitness .authorizePublish).publishAuthorized = false ∧
      (brokenCancelAuthorizeStep brokenCancelAuthorizationWitness
        .authorizePublish).publishAuthorized = true := by
  simp [brokenCancelAuthorizationWitness, step, brokenCancelAuthorizeStep]

theorem busy_remove_noop (state : State)
    (busy : state.writer = true ∨ 0 < state.readers) :
    step state .remove = state := by
  by_cases available : state.writer = false ∧ state.readers = 0
  · rcases busy with writer | readers
    · simp_all
    · omega
  · simp [step, available]

theorem successful_remove_clears_artifacts (state : State)
    (writer : state.writer = false) (readers : state.readers = 0) :
    (step state .remove).published = false ∧
      (step state .remove).«partial» = false := by
  simp [step, writer, readers]

theorem remove_idempotent (state : State) :
    step (step state .remove) .remove = step state .remove := by
  by_cases available : state.writer = false ∧ state.readers = 0 <;>
    simp [step, available]

theorem broken_remove_violates_idempotency :
    brokenRemoveStep stalePartialInitial .remove =
        step stalePartialInitial .remove ∧
      brokenRemoveStep (brokenRemoveStep stalePartialInitial .remove) .remove ≠
        brokenRemoveStep stalePartialInitial .remove := by
  simp [brokenRemoveStep, step, stalePartialInitial, publishedInitial]

end Yasumaro
