//! kqscript <disk1.do> <disk2.do> <disk3.do> [--out dir]
//! Disassembles every script (global script 0 and room scripts) to text, and validates
//! the structure: sections parse exactly and rule targets land on rule/action boundaries.
use kq1_apple2::kq::{DiskAddr, Disks};
use kq1_apple2::script::{Mode, RuleKind, Script};
use kq1_apple2::vocab::{self, Names};
use std::{collections::BTreeSet, env, fmt::Write as _, fs, path::PathBuf};

fn main() {
    let mut args: Vec<String> = env::args().skip(1).collect();
    let mut out = PathBuf::from("extracted/scripts");
    if let Some(p) = args.iter().position(|a| a == "--out") {
        out = PathBuf::from(&args[p + 1]);
        args.drain(p..p + 2);
    }
    if args.len() != 3 {
        eprintln!("usage: kqscript disk1.do disk2.do disk3.do [--out dir]");
        std::process::exit(2);
    }
    let disks = Disks::from_files(&args).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1)
    });
    fs::create_dir_all(&out).unwrap();
    let words = vocab::parse(disks.fixed(0x6787).expect("vocabulary").data).expect("parse vocabulary");
    let names = Names::new(&words);
    let mut vt = String::new();
    for e in &words {
        writeln!(vt, "{:<16} pos={} type={} id={}", e.word, e.position, e.kind, e.id).unwrap();
    }
    fs::write(out.join("vocabulary.txt"), vt).unwrap();
    let (mut count, mut issues) = (0, 0);
    for (i, rec) in disks.directory(0x1800).unwrap().iter().enumerate() {
        let Ok(res) = disks.read(DiskAddr::from_record(rec), 1) else { continue };
        let Ok(s) = Script::new(res.data) else { continue };
        let (text, problems) = dump(&s, &names);
        for p in &problems {
            println!("script {i:03}: {p}");
        }
        issues += problems.len();
        count += 1;
        fs::write(out.join(format!("{i:03}.txt")), text).unwrap();
    }
    println!("{count} scripts disassembled, {} vocabulary entries, {issues} issues", words.len());
}

fn dump(s: &Script, names: &Names) -> (String, Vec<String>) {
    let mut t = String::new();
    let mut problems = Vec::new();
    if s.bounds[4] != s.data.len() {
        problems.push(format!("header declares {} bytes, resource has {}", s.bounds[4], s.data.len()));
    }
    let acts = s.actions().unwrap_or_else(|e| {
        problems.push(format!("actions: {e}"));
        Vec::new()
    });
    // Valid action entry points: any opcode boundary or sequence end.
    let mut entries = BTreeSet::new();
    for (start, seq) in &acts {
        entries.insert(*start);
        entries.extend(seq.iter().map(|i| i.offset));
        entries.insert(seq.last().map_or(*start, |i| i.offset + 1 + i.args.len()));
    }
    for (mode, title) in [(Mode::Cycle, "CYCLE RULES"), (Mode::Parser, "PARSER RULES")] {
        writeln!(t, "== {title}").unwrap();
        match s.rules(mode) {
            Ok(rules) => {
                let bounds: BTreeSet<usize> = rules
                    .iter()
                    .map(|r| r.offset)
                    .chain([s.section(if mode == Mode::Cycle { 1 } else { 2 }).len()])
                    .collect();
                for r in &rules {
                    let cs = |c: &[kq1_apple2::script::Cond]| {
                        c.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(" && ")
                    };
                    let (line, target, action) = match &r.kind {
                        RuleKind::End => ("end".to_string(), None, None),
                        RuleKind::Goto { target } => (format!("goto {target:04X}"), Some(*target), None),
                        RuleKind::If { conds, target } => {
                            (format!("if {} else goto {target:04X}", cs(conds)), Some(*target), None)
                        }
                        RuleKind::IfEgoIn { rect, conds, target } => {
                            (format!("if ego in {rect:?} {} else goto {target:04X}", cs(conds)), Some(*target), None)
                        }
                        RuleKind::Said { verb, noun, conds, action } => (
                            format!(
                                "said({}, {}) {} -> A{action:04X}",
                                names.verb(*verb),
                                names.noun(*noun),
                                cs(conds)
                            ),
                            None,
                            Some(*action),
                        ),
                        RuleKind::Always { conds, action } => {
                            (format!("when {} -> A{action:04X}", cs(conds)), None, Some(*action))
                        }
                        RuleKind::EgoIn { rect, conds, action } => {
                            (format!("ego in {rect:?} {} -> A{action:04X}", cs(conds)), None, Some(*action))
                        }
                    };
                    writeln!(t, "  {:04X}  {line}", r.offset).unwrap();
                    if let Some(x) = target.filter(|x| !bounds.contains(x)) {
                        problems.push(format!("{title} {:04X}: target {x:04X} not a rule boundary", r.offset));
                    }
                    if let Some(a) = action.filter(|a| !entries.contains(a)) {
                        problems.push(format!("{title} {:04X}: action {a:04X} not an opcode boundary", r.offset));
                    }
                }
            }
            Err(e) => problems.push(format!("{title}: {e}")),
        }
    }
    writeln!(t, "== ACTIONS").unwrap();
    for (start, seq) in &acts {
        writeln!(t, "  A{start:04X}:").unwrap();
        for i in seq {
            match kq1_apple2::script::message_operand(i.op) {
                Some(k) => {
                    let off = i.args[k] as usize | (i.args[k + 1] as usize) << 8;
                    let shown = match s.message_at(off) {
                        Some(m) => format!("\"{}\"", m.replace('\r', "\\n")),
                        None => {
                            problems.push(format!(
                                "action {:04X}: message offset {off:04X} is not a message start",
                                i.offset
                            ));
                            format!("<bad message offset {off:04X}>")
                        }
                    };
                    let name = kq1_apple2::script::op_name(i.op).unwrap();
                    let lead: Vec<String> = i.args[..k].iter().map(|a| a.to_string()).collect();
                    writeln!(
                        t,
                        "    {:04X}  {name} {}{shown}",
                        i.offset,
                        lead.iter().map(|a| format!("{a} ")).collect::<String>()
                    )
                    .unwrap();
                }
                None => writeln!(t, "    {:04X}  {i}", i.offset).unwrap(),
            }
        }
    }
    writeln!(t, "== MESSAGES").unwrap();
    for (n, m) in s.messages().iter().enumerate() {
        writeln!(t, "  {:3}  {}", n + 1, m.replace('\r', "\\n")).unwrap();
    }
    (t, problems)
}
