// question: a bare call to a name that is both a builtin (spawnstruct) and a function of the same file: which one runs, and does the path-qualified call reach the script function?
// expect: f0 T0 script spawnstruct called
// expect: f0 T0 bare done
// expect: f0 T0 script spawnstruct called
// expect: f0 T0 qualified=42
// fact: a bare call reaches a function of the same file before the builtin of that name; the path-qualified call reaches it too
// claim: gscode-dialect-facts (disagrees): IW3 bare calls resolve builtin-first
// evidence: static: the call emitter (0x50ad90) first looks the name up in the current file's function map (0x50ac10: FindVariable 0x513d00 in 0x15020a8) and emits a script call when it is there; then the builtin cache (0x14e1078) and the builtin lookup (0x4d2f20); a name neither knows becomes a script call resolved over the #include'd files
// vs t4: same
// note: IW3 has no namespaces and no sys::; the qualified form is the file path

spawnstruct()
{
	logprint("script spawnstruct called");
	return 42;
}

main()
{
	a = spawnstruct();
	logprint("bare done");
	b = zz\bare_call_local_shadows_builtin::spawnstruct();
	logprint("qualified=" + b);
}
