import http from 'node:http';
import {spawn} from 'node:child_process';

const port=8787;
const workspace=process.env.COSMIC_WORKSPACE||process.cwd();
const send=(res,status,body)=>{res.writeHead(status,{'Content-Type':'application/json','Access-Control-Allow-Origin':'http://localhost:5173','Access-Control-Allow-Headers':'Content-Type'});res.end(JSON.stringify(body))};
const body=req=>new Promise((resolve,reject)=>{let raw='';req.on('data',chunk=>raw+=chunk);req.on('end',()=>{try{resolve(JSON.parse(raw||'{}'))}catch{reject(new Error('Invalid JSON'))}})});

function run(command,args,{timeout=15000,cwd=workspace}={}){return new Promise(resolve=>{let stdout='',stderr='',settled=false;const child=spawn(command,args,{cwd,env:process.env});const done=(code,timeoutHit=false)=>{if(settled)return;settled=true;clearTimeout(timer);resolve({code,stdout,stderr,timeout:timeoutHit})};const timer=setTimeout(()=>{child.kill('SIGTERM');setTimeout(()=>child.kill('SIGKILL'),1500);done(124,true)},timeout);child.stdout.on('data',data=>stdout+=data);child.stderr.on('data',data=>stderr+=data);child.on('error',error=>{stderr+=error.message;done(127)});child.on('close',code=>done(code??1))})}
const output=result=>result.stdout+result.stderr;
const has=(result,text)=>result.code===0&&output(result).includes(text);

async function providers(){
  const [codex,claude,opencode]=await Promise.all([
    run('codex',['login','status']),run('claude',['auth','status']),run('opencode',['auth','list'])
  ]);
  const list=[];
  if(has(codex,'Logged in'))list.push({id:'codex',name:'Codex',detail:'ChatGPT account connected',connected:true});
  try{if(claude.code===0&&JSON.parse(output(claude)).loggedIn)list.push({id:'claude',name:'Claude Code',detail:'Claude account connected',connected:true})}catch{}
  if(opencode.code===0&&/Credentials/.test(output(opencode)))list.push({id:'opencode',name:'OpenCode',detail:'Configured local provider',connected:true});
  // Grok is intentionally not exposed merely because its command is installed.
  // An explicit xAI key is required before Cosmic can send it any work.
  if(process.env.XAI_API_KEY)list.push({id:'grok',name:'Grok',detail:'xAI API key connected',connected:true});
  return list;
}

function textFromJsonl(raw){
  const events=raw.split('\n').flatMap(line=>{try{return[JSON.parse(line)]}catch{return[]}});
  const message=[...events].reverse().find(event=>event.item?.type==='agent_message'||event.type==='turn.completed');
  return message?.item?.text||message?.item?.content?.map?.(item=>item.text||'').join('')||message?.summary||'';
}

async function message({provider,message,sessionId}){
  let result,text='',nextSessionId=sessionId;
  if(provider==='codex'){
    const args=sessionId?['exec','resume','--json',sessionId,message]:['exec','--json','--sandbox','workspace-write','--cd',workspace,message];
    result=await run('codex',args,{timeout:600000});text=textFromJsonl(result.stdout);const event=result.stdout.split('\n').flatMap(line=>{try{return[JSON.parse(line)]}catch{return[]}}).find(item=>item.thread_id);nextSessionId=event?.thread_id||nextSessionId;
  }else if(provider==='claude'){
    const args=['-p',message,'--output-format','json','--permission-mode','plan','--permission-prompts','none'];if(sessionId)args.push('--resume',sessionId);result=await run('claude',args,{timeout:600000});try{const data=JSON.parse(result.stdout);text=data.result||'';nextSessionId=data.session_id||nextSessionId}catch{text=result.stdout}
  }else if(provider==='opencode'){
    const args=['run',message,'--format','json','--dir',workspace];if(sessionId)args.push('--session',sessionId);result=await run('opencode',args,{timeout:600000});text=textFromJsonl(result.stdout)||result.stdout;
  }else if(provider==='grok'){
    const args=['--single',message,'--output-format','json','--cwd',workspace,'--permission-mode','plan'];if(sessionId)args.push('--resume',sessionId);result=await run('grok',args,{timeout:600000});text=textFromJsonl(result.stdout)||result.stdout;
  }else throw new Error('This local agent is not connected.');
  if(result.timeout)throw new Error('The local agent timed out after 10 minutes.');
  if(result.code!==0)throw new Error(result.stderr.trim()||'The local agent could not complete this task.');
  return {text:text.trim()||'The agent completed the task.',sessionId:nextSessionId};
}

http.createServer(async(req,res)=>{
  if(req.method==='OPTIONS')return send(res,204,{});
  if(req.url==='/api/health'||req.url==='/api/providers')return send(res,200,{providers:await providers(),workspace});
  if(req.url==='/api/message'&&req.method==='POST'){
    try{const input=await body(req);if(typeof input.message!=='string'||!input.message.trim())return send(res,400,{error:'A message is required.'});const available=await providers();if(!available.some(item=>item.id===input.provider))return send(res,403,{error:'That local agent is not installed and authenticated.'});return send(res,200,await message(input))}catch(error){return send(res,502,{error:error.message||'The local agent could not be reached.'})}
  }
  return send(res,404,{error:'Not found'});
}).listen(port,()=>console.log(`Cosmic local-agent relay on http://localhost:${port} for ${workspace}`));
