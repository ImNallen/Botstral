import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import https from 'node:https';
import crypto from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { chromium } from '../../agent/runtime/node_modules/playwright/index.mjs';
import { login } from '../../agent/runtime/protected.mjs';
const root=await fs.mkdtemp(path.join(os.tmpdir(),'p06-final-independent-'));
execFileSync('openssl',['req','-x509','-newkey','rsa:2048','-nodes','-keyout',path.join(root,'key'),' -out'.trim(),path.join(root,'cert'),'-days','1','-subj','/CN=localhost'],{stdio:'ignore'});
let scenario, posts, validPosts, context;
const values={type:'password',username:'fresh-browser-private-identifier',password:'fresh-browser-private-password'};
const scripts={
  control:'',
  input_add_hidden:'document.querySelector("input").addEventListener("input",()=>{const x=document.createElement("input");x.type="hidden";x.name="csrf";x.value="unapproved";document.querySelector("form").append(x)})',
  formdata_replace_password:'document.querySelector("form").addEventListener("formdata",e=>e.formData.set("password","different payload"))',
  formdata_duplicate_username:'document.querySelector("form").addEventListener("formdata",e=>e.formData.append("username","extra"))',
  input_remove_readonly:'document.querySelector("input").addEventListener("input",()=>{document.querySelector("#password").readOnly=true})'
};
const server=https.createServer({key:await fs.readFile(path.join(root,'key')),cert:await fs.readFile(path.join(root,'cert'))},async(req,res)=>{
  if(req.method==='POST') {
    let data='';for await(const chunk of req)data+=chunk;
    posts++;const entries=[...new URLSearchParams(data)];
    if(JSON.stringify(entries)===JSON.stringify([['username',values.username],['password',values.password]]))validPosts++;
    res.end('Signed in');return;
  }
  res.setHeader('content-type','text/html');
  res.end('<form id="form" method="post" action="/session"><input id="username" name="username"><input id="password" name="password" type="password"><button id="submit" type="submit">Sign in</button></form><script>'+scripts[scenario]+'</script>');
});
await new Promise(r=>server.listen(0,'127.0.0.1',r));
const origin=`https://127.0.0.1:${server.address().port}`;
const results=[];
try {
 for(scenario of Object.keys(scripts)) {
  posts=0;validPosts=0;
  const parent=path.join(root,scenario), profile=path.join(parent,'profile');await fs.mkdir(profile,{recursive:true});
  const epoch=crypto.randomUUID();
  await fs.writeFile(path.join(profile,'.botstral-profile.json'),JSON.stringify({profile:epoch,releases:[]}));
  await fs.writeFile(path.join(parent,'automation-protection.json'),JSON.stringify({protection:{type:'tracked',profile:epoch,releases:[],complete:true}}));
  context=await chromium.launchPersistentContext(profile,{executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true,ignoreHTTPSErrors:true,serviceWorkers:'block'});
  const page=context.pages()[0];
  const release=crypto.randomUUID(),id=crypto.randomUUID();let settlement;
  const send=async op=>{
   if(op.type==='prepare')return {type:'review',review:{id,release,state:'authorized',expires_at:Math.floor(Date.now()/1000)+25}};
   if(op.type==='release')return {type:'released',release,values:{...values},valid_until:Math.floor(Date.now()/1000)+25};
   if(op.type==='settle'){settlement=op.applied;return {type:'review'};}
   throw Error('unexpected callback');
  };
  let completed=true;
  try {await login(page,{purpose:'password',plan:{version:1,origin,route:'/login',frame:null,form:'#form',username:'#username',password:'#password',totp:'#totp',submitter:'#submit',action:'/session'}},send,profile);}catch{completed=false;}
  await new Promise(r=>setTimeout(r,150));
  const expected=scenario==='control';
  assert.equal(completed,expected,scenario+' completion');assert.equal(posts,expected?1:0,scenario+' requests');assert.equal(validPosts,expected?1:0,scenario+' server validation');assert.equal(settlement,expected,scenario+' settlement');
  results.push({scenario,completed,posts,validPosts,settlement});
  await context.close();context=null;
 }
 console.log(JSON.stringify({boundary:'Production browser adapter; synthetic private authority callback; controlled HTTPS; not Linux transport',results}));
}finally{await context?.close();await new Promise(r=>server.close(r));await fs.rm(root,{recursive:true,force:true});}
