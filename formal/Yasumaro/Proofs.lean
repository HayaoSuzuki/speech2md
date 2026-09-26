import Yasumaro.SpeakerAssignment

namespace Yasumaro

theorem overlap_commutative (left right : TimeSpan) :
    overlapMs left right = overlapMs right left := by
  simp [overlapMs, Nat.min_comm, Nat.max_comm]

theorem overlap_zero_when_disjoint (left right : TimeSpan)
    (h : left.endMs ≤ right.startMs) : overlapMs left right = 0 := by
  simp [overlapMs]
  omega

theorem overlap_bounded_by_left (left right : TimeSpan) :
    overlapMs left right ≤ left.endMs - left.startMs := by
  simp [overlapMs]
  omega

theorem equal_overlap_selects_smaller_id (utterance : TimeSpan)
    (first second : SpeakerSegment)
    (threshold : OverlapThreshold)
    (equal : overlapMs utterance first.span = overlapMs utterance second.span)
    (smaller : second.speaker < first.speaker)
    (enough :
      meetsThreshold utterance (overlapMs utterance first.span) threshold = true) :
    assignSpeaker utterance [first, second] threshold =
      some second.speaker := by
  have enoughSecond :
      meetsThreshold utterance (overlapMs utterance second.span) threshold = true := by
    rw [← equal]
    exact enough
  simp [assignSpeaker, selectGreaterOverlap, equal, smaller, enoughSecond]

end Yasumaro
