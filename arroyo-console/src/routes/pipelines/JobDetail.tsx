import './pipelines.css';

import { useParams } from 'react-router-dom';
import {
  Alert,
  AlertDescription,
  AlertIcon,
  AlertTitle,
  Badge,
  Box,
  Button,
  ButtonGroup,
  Flex,
  Spacer,
  Stack,
  Tab,
  TabList,
  TabPanel,
  TabPanels,
  Tabs,
  Text,
  useDisclosure,
} from '@chakra-ui/react';
import React, { useState } from 'react';
import { GrpcOutputSubscription, OutputData, StopType } from '../../gen/api_pb';
import 'reactflow/dist/style.css';
import 'metrics-graphics/dist/mg.css';
import { PipelineGraph } from './JobGraph';
import { ApiClient } from '../../main';
import { PipelineOutputs } from './JobOutputs';
import { CodeEditor } from './SqlEditor';
import PipelineConfigModal from './PipelineConfigModal';
import { Code as ConnectWebCode, ConnectError } from '@bufbuild/connect-web';
import { useJob, useJobCheckpoints, useJobMetrics } from '../../lib/data_fetching';
import OperatorDetail from '../../components/OperatorDetail';
import JobNotFound from '../../components/JobNotFound';
import Checkpoints from '../../components/Checkpoints';
import { QuestionOutlineIcon, WarningIcon } from '@chakra-ui/icons';
import Loading from '../../components/Loading';

export function JobDetail({ client }: { client: ApiClient }) {
  const [activeOperator, setActiveOperator] = useState<string | undefined>(undefined);
  const [outputs, setOutputs] = useState<Array<{ id: number; data: OutputData }>>([]);
  const [subscribed, setSubscribed] = useState<boolean>(false);
  const {
    isOpen: configModalOpen,
    onOpen: onConfigModalOpen,
    onClose: onConfigModalClose,
  } = useDisclosure();

  let { id } = useParams();

  const { job, jobError, updateJob } = useJob(client, id);
  const { metrics } = useJobMetrics(client, id);
  const { checkpoints } = useJobCheckpoints(client, id);

  if (jobError) {
    if (jobError instanceof ConnectError && jobError.code === ConnectWebCode.NotFound) {
      return <JobNotFound icon={<QuestionOutlineIcon boxSize={55} />} message={'Job not found'} />;
    }

    console.error('Failed to fetch job', jobError);
    return <JobNotFound icon={<WarningIcon boxSize={55} />} message={'Error fetching job'} />;
  }

  const subscribe = async () => {
    if (subscribed) {
      return;
    }

    setSubscribed(true);

    let row = 1;
    for await (const res of (await client()).subscribeToOutput(
      new GrpcOutputSubscription({
        jobId: id,
      })
    )) {
      outputs.push({ id: row++, data: res });
      if (outputs.length > 20) {
        outputs.shift();
      }

      setOutputs(outputs);
    }
  };

  async function updateJobState(stop: StopType) {
    console.log(`Setting pipeline stop_mode=${stop}`);
    updateJob({ stop });
  }

  async function updateJobParallelism(parallelism: number) {
    console.log(`Setting pipeline parallelism=${parallelism}`);
    updateJob({ parallelism });
  }

  let inner = <Loading />;

  if (job?.jobStatus && job?.jobGraph) {
    let operatorDetail = undefined;
    if (activeOperator) {
      operatorDetail = (
        <OperatorDetail operator_id={activeOperator} graph={job?.jobGraph} metrics={metrics} />
      );
    }

    const operatorsTab = (
      <TabPanel h="100%">
        <Flex h="100%">
          <Box flex="1">
            <PipelineGraph
              graph={job.jobGraph}
              metrics={metrics}
              setActiveOperator={setActiveOperator}
              activeOperator={activeOperator}
            />
          </Box>
          <Stack w="500px" className="pipelineInfo" spacing={2}>
            {job?.jobStatus?.failureMessage ? (
              <Alert status="error" marginBottom={5}>
                <Box>
                  <AlertTitle>Job Failed</AlertTitle>
                  <AlertDescription>{job?.jobStatus?.failureMessage}</AlertDescription>
                </Box>
                <Spacer />
                <AlertIcon alignSelf="flex-start" />
              </Alert>
            ) : null}

            <Box className="field">
              <Box className="fieldName">Name</Box>
              <Box className="fieldValue">{job.jobStatus?.pipelineName}</Box>
            </Box>
            <Box className="field">
              <Box className="fieldName">State</Box>
              <Box className="fieldValue">{job.jobStatus?.state}</Box>
            </Box>
            {operatorDetail}
          </Stack>
        </Flex>
      </TabPanel>
    );

    const outputsTab = (
      <TabPanel>
        {outputs.length == 0 ? (
          job?.jobGraph?.nodes.find(n => n.operator.includes('GrpcSink')) != null ? (
            <Button isLoading={subscribed} onClick={subscribe} width={150} size="sm">
              Read output
            </Button>
          ) : (
            <Text>Pipeline does not have a web sink</Text>
          )
        ) : (
          <PipelineOutputs outputs={outputs} />
        )}
      </TabPanel>
    );

    const checkpointsTab = (
      <TabPanel>
        {<Checkpoints client={client} job={job} checkpoints={checkpoints?.checkpoints ?? []} />}
      </TabPanel>
    );

    const queryTab = (
      <TabPanel>
        <Box>
          <CodeEditor query={job?.jobStatus?.definition!} readOnly={true} />
        </Box>
      </TabPanel>
    );

    const udfsTab = (
      <TabPanel>
        <Box>
          <CodeEditor
            query={(job?.jobStatus?.udfs || [{ definition: '' }])[0].definition}
            language="rust"
            readOnly={true}
          />
        </Box>
      </TabPanel>
    );

    inner = (
      <Tabs h="100%">
        <TabList>
          <Tab>Operators</Tab>
          <Tab>Outputs</Tab>
          <Tab>Checkpoints</Tab>
          <Tab>Query</Tab>
          <Tab>UDFs</Tab>
        </TabList>
        <TabPanels h="100%">
          {operatorsTab}
          {outputsTab}
          {checkpointsTab}
          {queryTab}
          {udfsTab}
        </TabPanels>
      </Tabs>
    );
  }

  let configModal = <></>;
  if (job?.jobGraph?.nodes) {
    const { nodes } = job?.jobGraph;
    const parallelism = Math.max(...nodes.map(({ parallelism }) => parallelism));

    configModal = (
      <PipelineConfigModal
        isOpen={configModalOpen}
        parallelism={parallelism}
        onClose={onConfigModalClose}
        updateJobParallelism={updateJobParallelism}
      />
    );
  }

  let editPipelineButton = <></>;
  let actionButton = <></>;
  if (job) {
    editPipelineButton = <Button onClick={onConfigModalOpen}>Edit</Button>;
    actionButton = (
      <Button
        isDisabled={job.action == undefined}
        isLoading={job.inProgress}
        loadingText={job.actionText}
        onClick={async () => {
          await updateJobState(job.action!);
        }}
      >
        {job.actionText}
      </Button>
    );
  }

  return (
    <Box top={0} bottom={0} right={0} left={200} position="absolute" overflowY="hidden">
      <Flex>
        <Box p={5}>
          <Text fontSize={20}>
            Pipeline {id} <Badge>{job?.jobStatus?.state}</Badge>
          </Text>
        </Box>
        <Spacer />
        <Box p={5}>
          <ButtonGroup>
            {editPipelineButton}
            {actionButton}
          </ButtonGroup>
        </Box>
      </Flex>
      {inner}
      {configModal}
    </Box>
  );
}
