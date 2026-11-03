import * as driver from '../../channel-drivers/signal.mjs';
export default function register(api){
 api.registerChannel({...driver,async diagnose(){
  return {verified:false,httpStatus:null,message:'Signal bridge is configured; runtime WebSocket status requires live connection'};
 }});
}
