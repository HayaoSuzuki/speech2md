import Std.Tactic

namespace Yasumaro

structure TimeSpan where
  startMs : Nat
  endMs : Nat
  valid : startMs ≤ endMs
deriving Repr

instance : BEq TimeSpan where
  beq left right :=
    left.startMs == right.startMs && left.endMs == right.endMs

def TimeSpan.create (startMs endMs : Nat) : Option TimeSpan :=
  if h : startMs ≤ endMs then
    some ⟨startMs, endMs, h⟩
  else
    none

def TimeSpan.durationMs (span : TimeSpan) : Nat :=
  span.endMs - span.startMs

structure OverlapThreshold where
  numerator : Nat
  denominator : Nat
  denominatorPositive : 0 < denominator
  atMostOne : numerator ≤ denominator
deriving Repr

instance : BEq OverlapThreshold where
  beq left right :=
    left.numerator == right.numerator && left.denominator == right.denominator

def OverlapThreshold.create (numerator denominator : Nat) :
    Option OverlapThreshold :=
  if positive : 0 < denominator then
    if bounded : numerator ≤ denominator then
      some ⟨numerator, denominator, positive, bounded⟩
    else
      none
  else
    none

structure SpeakerSegment where
  speaker : Nat
  span : TimeSpan
deriving BEq, Repr

def overlapMs (left right : TimeSpan) : Nat :=
  min left.endMs right.endMs - max left.startMs right.startMs

def meetsThreshold (utterance : TimeSpan) (overlap : Nat)
    (threshold : OverlapThreshold) : Bool :=
  threshold.numerator == 0 ||
    (0 < utterance.durationMs &&
      utterance.durationMs * threshold.numerator ≤
        overlap * threshold.denominator)

def selectGreaterOverlap (utterance : TimeSpan)
    (best candidate : SpeakerSegment) : SpeakerSegment :=
  let candidateOverlap := overlapMs utterance candidate.span
  let bestOverlap := overlapMs utterance best.span
  if candidateOverlap > bestOverlap ∨
      (candidateOverlap = bestOverlap ∧ candidate.speaker < best.speaker) then
    candidate
  else
    best

def assignSpeaker (utterance : TimeSpan) (segments : List SpeakerSegment)
    (threshold : OverlapThreshold) : Option Nat :=
  match segments with
  | [] => none
  | first :: rest =>
      let best := rest.foldl (selectGreaterOverlap utterance) first
      if meetsThreshold utterance (overlapMs utterance best.span) threshold then
        some best.speaker
      else
        none

end Yasumaro
