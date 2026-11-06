// question: a bare call to a function that only a #using'd file of another namespace defines: does the game's linker find it?
// fact: a bare call never reaches another namespace, even a #using'd one: the game's linker reports an unresolved external and the load is a fatal Sys_Error
// vs t7: T7 reports the same unresolved external through Com_Error; T8 ends in Sys_Error 0x6394f836
// expect error: Sys_Error 0x6394f836
// with: zz_bare_lib.gsc
#using scripts\zz_harness\zz_bare_lib;
#namespace zz_test;

function main()
{
	a = lib_only();
	iprintln("lib_only=" + a);
}
