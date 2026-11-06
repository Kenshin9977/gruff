// question: a bare call to a name that is both a builtin (spawnstruct) and a function of the same file: which one runs?
// fact: the file's own function: a bare call reaches a function of the same file before a builtin of the same name
// vs t6: the builtin (T6's linker tries the builtins first)
// expect: f0 T0 script spawnstruct called
// expect: f0 T0 bare is int=1
// expect: f0 T0 script spawnstruct called
// expect: f0 T0 qualified=42

spawnstruct()
{
	logprint("script spawnstruct called");
	return 42;
}

main()
{
	a = spawnstruct();
	logprint("bare is int=" + isint(a));
	b = zz_harness\bare_call_local_shadows_builtin::spawnstruct();
	logprint("qualified=" + b);
}
