function ProgressBar(currentStep, totalSteps, time)
    if ~strcmp(getenv('MKS_PROGRESS'), '1')
        return
    end

    % Calculate the progress percentage
    progressPercentage = 100 * currentStep / totalSteps;
    
    % Print header only once
    if currentStep==1
        fprintf('\n'); % Extra line for better visibility
        fprintf('%-12s %-15s %-12s\n', 'Current Step', 'Current Time', 'Progress (%)');
        fprintf('%-12s %-15s %-12s\n', '------------', '-------------', '------------');
    end
    
    % Check if this is one of the selected steps to print
    if currentStep==1 || mod(currentStep, 20) == 0 || currentStep == totalSteps
        % Print the current step, time, and progress, aligning text to the left
        fprintf('%-12d %-15.2e %-12.2f\n', currentStep, time, progressPercentage);
    end
end
