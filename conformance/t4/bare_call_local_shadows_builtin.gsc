// question: a bare call to a name that is both a builtin (spawnstruct) and a function of the same file: which one runs, and does the path-qualified call reach the script function?
// expect: f0 T0 script spawnstruct called
// expect: f0 T0 bare done
// expect: f0 T0 script spawnstruct called
// expect: f0 T0 qualified=42
// vs t7: same order (T7: the current namespace before the builtin)
// fact: a bare call reaches a function of the same file before the builtin of that name; the path-qualified call reaches it too
// claim: gscode-dialect-facts (disagrees): T4 bare calls resolve builtin-first
// evidence: static: the call emitter (0x682ca0) first searches the current file's function map (0x3701fe8 + inst*0x1d8, FindVariable 0x690260 at 0x682cf2) and emits a script call when the name is there; only then the builtin cache (0x36bff84) and the builtin lookup (0x5676f0 for inst 0); a name neither knows becomes a script call resolved against the #include'd files ('unknown function' when absent)
// note: T4 has no namespaces and no sys::; the qualified form is the file path

spawnstruct()
{
	iprintln("script spawnstruct called");
	return 42;
}

main()
{
	a = spawnstruct();
	iprintln("bare done");
	b = zz\bare_call_local_shadows_builtin::spawnstruct();
	iprintln("qualified=" + b);
}
