// A small zombies-like round loop written for the trace demo (docs/trace-format.md). No
// entities: the zombies are structs. Rounds spawn round+1 zombies, each dies after a delay and
// tells the round watcher; round 3 starts a nuke that kills the zombies still alive through
// their endon("death"); "game_over" ends every loop through endon. Run it in the emulated BO3
// VM and view the timeline:
//   py -3.11 -I tools/re/vm/t7_run.py tools/re/vm/examples/round_loop.gsc --trace rl.jsonl
//   py -3.11 -I tools/re/vm/trace_view.py rl.jsonl
#namespace zm_sim;

function main()
{
	level.round = 0;
	level.alive = 0;
	level.zombies = [];
	level thread round_watcher();
	level thread powerup_watcher();
	level thread round_loop();
	wait 2;
	level notify("game_over");
	logprint("game over in round " + level.round);
}

function round_loop()
{
	level endon("game_over");
	while (true)
	{
		level.round++;
		logprint("round " + level.round);
		level notify("start_of_round", level.round);
		for (i = 0; i < level.round + 1; i++)
		{
			z = spawnstruct();
			z.id = level.round * 10 + i;
			level.zombies[level.zombies.size] = z;
			level.alive++;
			z thread zombie_think(i);
			wait 0.05;
		}
		level waittill("round_cleared");
		wait 0.1;
	}
}

function zombie_think(i)
{
	self endon("death");
	level endon("game_over");
	wait 0.1 * (i + 1);
	self.dead = true;
	level.alive--;
	level notify("zombie_died", self.id);
}

function round_watcher()
{
	level endon("game_over");
	while (true)
	{
		level waittill("zombie_died", id);
		logprint("zombie " + id + " died, " + level.alive + " left");
		if (level.alive == 0)
		{
			level notify("round_cleared");
		}
	}
}

function powerup_watcher()
{
	level endon("game_over");
	level waittillmatch("start_of_round", 3);
	wait 0.1;
	logprint("nuke");
	foreach (z in level.zombies)
	{
		if (!isdefined(z.dead))
		{
			z.dead = true;
			level.alive--;
			z notify("death");
		}
	}
	if (level.alive == 0)
	{
		level notify("round_cleared");
	}
}
