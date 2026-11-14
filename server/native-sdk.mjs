import {DominionSDK} from './agent-sdk.mjs';
import {newSession,runTurn,sessionMessages,cancelExecution} from './engine.mjs';
import {get} from './store.mjs';
import {createGoal,goal,goals,resumeGoal,cancelGoal,goalEvents} from './harness.mjs';
import {enqueue} from './jobs.mjs';

// The SDK uses DOMINION's existing model, MCP, approval, audit and SQLite engines.
// It does not launch another vendor's CLI or replace the product harness.
export const dominionSdk=new DominionSDK({
 createSession:newSession,
 getSession:id=>get('session',id),
 readMessages:sessionMessages,
 runTurn,
 getAgent:id=>get('agent',id),
 getExecution:id=>get('execution',id),
 cancel:cancelExecution,
 createGoal,getGoal:goal,listGoals:goals,resumeGoal,cancelGoal,getGoalEvents:goalEvents,enqueue
});
