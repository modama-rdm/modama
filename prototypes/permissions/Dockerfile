FROM debian:stable-slim
LABEL Dieter Weber <d.weber@fz-juelich.de>

RUN apt-get -y update && apt-get -y upgrade && apt-get -y install sudo acl inotify-tools

RUN groupadd testerc && groupadd testholo && groupadd testprivate && groupadd testshare

RUN adduser testdieter --gecos "" --no-create-home --disabled-login --ingroup testerc && adduser testdieter testholo && adduser testdieter testshare
RUN adduser testalex --gecos "" --no-create-home --disabled-login --ingroup testerc && adduser testalex testholo && adduser testalex testshare
RUN adduser testthibaud --gecos "" --no-create-home --disabled-login --ingroup testerc && adduser testthibaud testholo && adduser testthibaud testprivate 
RUN adduser testpenghan --gecos "" --no-create-home --disabled-login --ingroup testerc && adduser testpenghan testholo && adduser testpenghan testprivate
RUN adduser testjanghyun --gecos "" --no-create-home --disabled-login --ingroup testerc && adduser testjanghyun testprivate && adduser testjanghyun testshare

COPY test.sh /test.sh
COPY watchdog.sh /watchdog.sh

CMD /test.sh
