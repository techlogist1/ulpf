<script>
  // Trust flags: the stages that did not reach their outcome for one event. Never a score and
  // never coloured as one - every mark is warn, because every mark is the same kind of fact.
  // Two letters in the row (the column is 7em), the full flag and what it means in the title.
  let { flags = [] } = $props()

  const WHY = {
    no_parser: ['np', 'no definition claimed the event'],
    parse_failed: ['pf', 'a definition claimed the event and failed to parse it'],
    sub_uncovered: ['su', 'a message id no sub pattern covers yet'],
    sub_no_match: ['sn', 'a gated sub pattern ran and failed'],
    time_from_receipt: ['tr', 'no device time was found: the time is the receipt time'],
    time_error: ['te', 'the timestamp text was found but did not parse'],
    class_unknown: ['cu', 'no class rule matched the fields'],
    unmapped: ['um', 'source fields no mapping rule consumed'],
    utf8_lossy: ['u8', 'the output text is not the exact bytes'],
  }

  const marks = $derived(
    flags.map((f) => {
      const i = f.indexOf(':')
      const base = i < 0 ? f : f.slice(0, i)
      const arg = i < 0 ? '' : f.slice(i + 1)
      const [m, why] = WHY[base] ?? [base.slice(0, 2), '']
      // A count belongs on the mark (um3); a reason is too long for the column and stays in the title.
      return { text: /^\d+$/.test(arg) ? `${m}${arg}` : m, title: why ? `${f}\n${why}` : f }
    }),
  )
</script>

<span class="flags">{#each marks as k}<span class="tag warn" title={k.title}>{k.text}</span>{/each}</span>
